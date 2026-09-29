"""Shared fixtures and API interception for the extensions scenarios.

The extensions route coverage is split across ``scenarios/test_extensions_*``
modules; this module (deliberately not named ``test_*``) holds the fixture
payloads and ``page.route()`` interception they share.

Scenario: Extensions route — focused UI coverage (SolidJS).

Ported from the legacy 30+ case suite to a focused ~14-case set covering each
surface once. All extension/pairing APIs are intercepted with `page.route()`
so no real WASM binaries or registry connections are needed (identical strategy
to the legacy test, retargeted at the SolidJS DOM).

Key adaptations from the legacy shell (and dropped micro-cases):
  - Configure is an inline `.extensions-detail` aside, not a `.configure-modal`
    overlay. There is no backdrop-click-to-close, no OAuth-on-save, no
    save-failure toast (the SolidJS setup mutation just refetches). Dropped:
    backdrop-click close, Enter-to-submit, save-OAuth popup, save-failure
    toast, "stays open on failure" regression, field optional/auto-generate
    badge variants (the SolidJS panel renders prompts + a "provided" tick only).
  - Remove uses a Kobalte AlertDialog (`.extensions-remove-dialog`), not
    `window.confirm`.
  - Registry entries render in one WASM table (display_name, description,
    Install); keywords are not shown, so the keyword micro-case is dropped.
  - Install/activate failures surface no toast in the SolidJS UI, so the
    error-toast and OAuth-popup/`auth_url`-injection cases are dropped (the
    URL-injection guard now lives in chat-cards `isHttpUrl`).
  - Auth cards moved to the chat surface; their coverage lives in test_chat.py.
  - Tab-reload / auth_completed-reload cases are TanStack Query internals and
    are dropped.
"""

import json

from helpers import goto_route

# --- Fixture data ---------------------------------------------------------

WASM_TOOL = {
    "name": "test-tool",
    "display_name": "Test WASM Tool",
    "kind": "wasm_tool",
    "description": "A test WASM tool extension",
    "url": None,
    "active": True,
    "authenticated": True,
    "has_auth": True,
    "needs_setup": False,
    "tools": ["search", "fetch"],
    "activation_status": None,
    "activation_error": None,
    "version": "1.0.0",
}

MCP_ACTIVE = {
    "name": "test-mcp",
    "display_name": "Test MCP Server",
    "kind": "mcp_server",
    "description": "An active MCP server",
    "url": "http://localhost:3000",
    "active": True,
    "authenticated": False,
    "has_auth": False,
    "needs_setup": False,
    "tools": [],
    "activation_status": None,
    "activation_error": None,
}

MCP_INACTIVE = {
    **MCP_ACTIVE,
    "name": "test-mcp-inactive",
    "display_name": "Inactive MCP",
    "active": False,
}

WASM_CHANNEL = {
    "name": "test-channel",
    "display_name": "Test Channel",
    "kind": "wasm_channel",
    "description": "A test WASM channel",
    "url": None,
    "active": False,
    "authenticated": False,
    "has_auth": False,
    "needs_setup": True,
    "tools": [],
    "activation_status": "installed",
    "activation_error": None,
}

REGISTRY_WASM = {
    "name": "registry-tool",
    "display_name": "Registry Tool",
    "kind": "wasm_tool",
    "description": "A registry WASM tool",
    "keywords": ["search", "utility"],
    "installed": False,
}

SAMPLE_TOOLS = [
    {"name": "echo", "description": "Echo a message"},
    {"name": "time", "description": "Get current time"},
]


# --- Interception helpers -------------------------------------------------


async def fulfil(route, payload):
    await route.fulfill(
        status=200, content_type="application/json", body=json.dumps(payload)
    )


async def mock_ext_apis(page, *, installed=None, tools=None, registry=None):
    """Intercept the extension list/tools/registry + a default empty pairing.

    Must be called BEFORE navigating to the extensions route.
    """
    await page.route(
        "**/api/extensions", lambda r: fulfil(r, {"extensions": installed or []})
    )
    await page.route(
        "**/api/extensions/tools", lambda r: fulfil(r, {"tools": tools or []})
    )
    await page.route(
        "**/api/extensions/registry*",
        lambda r: fulfil(r, {"entries": registry or []}),
    )

    async def handle_pairing(route):
        # Default: no pending pairing requests for any channel.
        await fulfil(route, {"channel": "test-channel", "requests": []})

    await page.route("**/api/pairing/**", handle_pairing)


async def go_to_extensions(page):
    await goto_route(page, "Extensions", "extensions", timeout=8000)


def registry_panel(page):
    return page.locator(".catalogue-panel", has=page.locator("#extensions-registry-search"))


def mcp_panel(page):
    return page.locator(".catalogue-panel", has=page.locator("#mcp-server-name"))


def tools_tbody(page):
    return page.locator(".catalogue-section--bare .catalogue-list--extensions tbody")
