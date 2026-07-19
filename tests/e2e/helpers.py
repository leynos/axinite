"""Shared helpers for E2E tests (SolidJS UI).

The Python Playwright suite drives the SolidJS single-page app served by the
real daemon. The legacy handwritten shell (tab bar, approval overlay, global
``addMessage``/``showApproval``/``connectSSE`` functions) is gone; every
selector below targets the SolidJS DOM described by the testability contract in
``docs/execplans/adopt-solidjs-ui-followups.md`` (section F7).

Selector conventions
--------------------
Prefer, in order: ``data-testid`` attributes, ARIA role/label lookups
(``page.get_by_role`` / ``page.get_by_label`` in the scenarios), then stable
component class names. The ``SEL`` map holds CSS/testid strings; scenarios use
``page.get_by_role``/``get_by_label`` directly where a role or accessible name
is the more robust anchor (each such use is commented at the call site).
"""

import asyncio
import re
import time

import httpx

# -- DOM Selectors --------------------------------------------------------
# One place for every selector so a frontend change needs a single update.

SEL = {
    # --- Auth gate (components/auth-gate.tsx) ---------------------------
    # `#auth-screen` exists only while checking or locked; it is absent once
    # the app unlocks (Playwright state="hidden" matches absence).
    "auth_screen": "#auth-screen",
    "auth_token_input": "#auth-gate-token",  # password field on the lock form
    "auth_submit": ".auth-gate__panel button[type='submit']",
    # --- App shell (components/app-shell.tsx) --------------------------
    # role=navigation via aria-label "Primary navigation".
    "nav": "nav.shell-nav",
    # Connection indicator: data-state in idle|connecting|connected|disconnected
    "sse_status": "[data-testid='sse-status']",
    # --- Chat (components/chat-preview.tsx) ----------------------------
    # textarea aria-label "Message composer"; Send button disabled when empty.
    "chat_composer": ".chat-preview__textarea",
    "chat_send": ".chat-preview__send-button",
    "chat_conversation": ".chat-preview__conversation",
    # Message turns carry data-role="user"/"assistant" on the turn wrapper.
    "chat_turn_user": "[data-role='user']",
    "chat_turn_assistant": "[data-role='assistant']",
    # Assistant markdown rendered by the app's sanitizing renderer.
    "chat_markdown": ".chat-preview__markdown",
    # --- Approval card (chat-preview.tsx, driven by history query) ------
    # Rendered from GET /api/chat/history's pending_approval field. The card
    # body carries [data-request-id]; buttons are Approve/Always/Deny (role).
    "approval_card": ".chat-preview__markdown[data-request-id]",
    # --- Auth card (components/chat-cards.tsx, SSE-driven) --------------
    "auth_card": ".chat-preview__auth-card",
    "auth_card_title": ".chat-preview__auth-card-title",
    "auth_card_instructions": ".chat-preview__auth-card-instructions",
    "auth_card_input": ".chat-preview__auth-card-input",
    "auth_card_submit": ".chat-preview__auth-card-submit",
    "auth_card_cancel": ".chat-preview__auth-card-cancel",
    "auth_card_oauth": ".chat-preview__auth-card-oauth",
    "auth_card_setup": ".chat-preview__auth-card-setup",
    "auth_card_error": ".chat-preview__auth-card-error",
    "auth_notice": ".chat-preview__auth-notice",
    # --- Extensions (components/extensions-preview.tsx) ----------------
    "ext_card": ".extensions-card",
    "ext_card_title": ".catalogue-card__title",
    # Relative to a card locator (the kind pill lives in the title-wrap header).
    "ext_card_kind": ".catalogue-card__title-wrap .pill",
    "ext_active_dot": ".catalogue-status-dot--active",
    "ext_action": ".catalogue-card__action",  # Configure/Activate/Disable/Remove
    "ext_tags": ".catalogue-card__tags",
    # Configure inline panel (aside), not a modal. The child selectors are
    # relative — scope them under `ext_configure_panel` in the scenarios.
    "ext_configure_panel": ".extensions-detail",
    "ext_configure_input": ".catalogue-form__input",
    "ext_configure_provided_icon": ".catalogue-form__provided-icon",
    "ext_configure_empty": ".catalogue-panel__empty",
    # Remove confirmation: Kobalte AlertDialog (portalled to <body>).
    "ext_remove_dialog": ".extensions-remove-dialog",
    "ext_remove_overlay": ".dialog-overlay",
    # WASM registry table (imperatively rendered rows).
    "ext_registry_search": "#extensions-registry-search",
    "ext_registry_row": ".catalogue-list--extensions .catalogue-list__row",
    "ext_registry_action": ".catalogue-list__action .catalogue-card__action",
    # MCP servers panel + tools table.
    "ext_mcp_empty": ".catalogue-panel__empty",
    "ext_tools_body": ".catalogue-list--extensions tbody",
    # WASM channel activation stepper (components/wasm-channel-stepper.tsx).
    "ext_stepper": ".ext-stepper",
    "ext_stepper_step": ".stepper-step",
    "ext_stepper_circle": ".stepper-circle",
    # Pairing rows (components/extension-pairing.tsx).
    "ext_pairing": ".ext-pairing",
    "ext_pairing_row": ".pairing-row",
    "ext_pairing_code": ".pairing-code",
    "ext_pairing_error": ".pairing-row__error",
    # --- Skills (components/skills-preview.tsx) ------------------------
    # Search input lives in the dedicated search section (the URL/upload
    # panels reuse `.skills-search__row`, so scope by the search section).
    "skill_search_input": ".skills-section--search input[type='text']",
    "skill_search_result": ".skills-search__result",
    "skill_installed_card": ".skills-card",
    # --- Logs (components/logs-preview.tsx) ----------------------------
    "logs_level_select": "#logs-level",
    "logs_filter_level": "#logs-filter-level",
    "logs_filter_target": "#logs-filter-target",
    "logs_panel": ".logs-panel",
}

# Ordered nav routes: (accessible link name, url path segment).
# Mirrors ROUTE_ORDER in web-src/axinite/src/lib/route-config.ts.
ROUTES = [
    ("Chat", "chat"),
    ("Memory", "memory"),
    ("Jobs", "jobs"),
    ("Routines", "routines"),
    ("Extensions", "extensions"),
    ("Skills", "skills"),
    ("Logs", "logs"),
]

# Per-route landmark that renders inside <main> once the route is active. The
# jobs/routines/logs routes share the `route-preview--dashboard` root, so those
# are scoped by the shell's unique `data-route` (the router pathname).
ROUTE_LANDMARK = {
    "chat": ".route-preview--chat",
    "memory": ".route-preview--memory",
    "jobs": "main[data-route='/jobs'] .route-preview--dashboard",
    "routines": "main[data-route='/routines'] .route-preview--dashboard",
    "extensions": ".route-preview--extensions",
    "skills": ".route-preview--skills",
    "logs": "main[data-route='/logs'] .route-preview--dashboard",
}

# Auth token used across all tests.
AUTH_TOKEN = "e2e-test-token"


async def goto_route(page, name: str, path: str, *, timeout: int = 5000):
    """Click a shell nav link and wait for its route landmark to appear."""
    # role=link named after the localized route label (en-GB default).
    await page.get_by_role("link", name=name, exact=True).click()
    await page.wait_for_url(f"**/{path}", timeout=timeout)
    await page.locator(ROUTE_LANDMARK[path]).first.wait_for(
        state="visible", timeout=timeout
    )


async def wait_for_ready(url: str, *, timeout: float = 60, interval: float = 0.5):
    """Poll a URL until it returns 200 or timeout."""
    deadline = time.monotonic() + timeout
    async with httpx.AsyncClient() as client:
        while time.monotonic() < deadline:
            try:
                resp = await client.get(url, timeout=5)
                if resp.status_code == 200:
                    return
            except (httpx.ConnectError, httpx.ReadError, httpx.TimeoutException):
                pass
            await asyncio.sleep(interval)
    raise TimeoutError(f"Service at {url} not ready after {timeout}s")


async def wait_for_port_line(process, pattern: str, *, timeout: float = 60) -> int:
    """Read process stdout line by line until a port-bearing line matches."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        try:
            line = await asyncio.wait_for(process.stdout.readline(), timeout=remaining)
        except asyncio.TimeoutError:
            break
        decoded = line.decode("utf-8", errors="replace").strip()
        if match := re.search(pattern, decoded):
            return int(match.group(1))
    raise TimeoutError(f"Port pattern '{pattern}' not found in stdout after {timeout}s")
