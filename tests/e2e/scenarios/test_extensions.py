"""Scenario: Extensions route — focused UI coverage (SolidJS).

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

from helpers import SEL, goto_route

# --- Fixture data ---------------------------------------------------------

_WASM_TOOL = {
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

_MCP_ACTIVE = {
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

_MCP_INACTIVE = {
    **_MCP_ACTIVE,
    "name": "test-mcp-inactive",
    "display_name": "Inactive MCP",
    "active": False,
}

_WASM_CHANNEL = {
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

_REGISTRY_WASM = {
    "name": "registry-tool",
    "display_name": "Registry Tool",
    "kind": "wasm_tool",
    "description": "A registry WASM tool",
    "keywords": ["search", "utility"],
    "installed": False,
}

_SAMPLE_TOOLS = [
    {"name": "echo", "description": "Echo a message"},
    {"name": "time", "description": "Get current time"},
]


# --- Interception helpers -------------------------------------------------


async def _fulfil(route, payload):
    await route.fulfill(
        status=200, content_type="application/json", body=json.dumps(payload)
    )


async def mock_ext_apis(page, *, installed=None, tools=None, registry=None):
    """Intercept the extension list/tools/registry + a default empty pairing.

    Must be called BEFORE navigating to the extensions route.
    """
    await page.route(
        "**/api/extensions", lambda r: _fulfil(r, {"extensions": installed or []})
    )
    await page.route(
        "**/api/extensions/tools", lambda r: _fulfil(r, {"tools": tools or []})
    )
    await page.route(
        "**/api/extensions/registry*",
        lambda r: _fulfil(r, {"entries": registry or []}),
    )

    async def handle_pairing(route):
        # Default: no pending pairing requests for any channel.
        await _fulfil(route, {"channel": "test-channel", "requests": []})

    await page.route("**/api/pairing/**", handle_pairing)


async def go_to_extensions(page):
    await goto_route(page, "Extensions", "extensions", timeout=8000)


def _registry_panel(page):
    return page.locator(".catalogue-panel", has=page.locator("#extensions-registry-search"))


def _mcp_panel(page):
    return page.locator(".catalogue-panel", has=page.locator("#mcp-server-name"))


def _tools_tbody(page):
    return page.locator(".catalogue-section--bare .catalogue-list--extensions tbody")


# --- Group A: structural / empty state ------------------------------------


async def test_extensions_empty_state(page):
    """No installed cards; MCP panel empty message; empty tools table."""
    await mock_ext_apis(page)
    await go_to_extensions(page)

    assert await page.locator(SEL["ext_card"]).count() == 0
    mcp = _mcp_panel(page)
    assert "No MCP servers available" in await mcp.text_content()
    assert await _tools_tbody(page).locator("tr").count() == 0


async def test_tools_table_populated(page):
    """Two mock tools produce two rows in the tools table."""
    await mock_ext_apis(page, tools=_SAMPLE_TOOLS)
    await go_to_extensions(page)

    tbody = _tools_tbody(page)
    await tbody.locator("tr").first.wait_for(state="visible", timeout=5000)
    assert await tbody.locator("tr").count() == 2
    text = await tbody.text_content()
    assert "echo" in text and "time" in text


# --- Group B: installed cards ---------------------------------------------


async def test_installed_wasm_tool_card(page):
    """An active WASM tool card shows name, kind pill, active dot, remove."""
    await mock_ext_apis(page, installed=[_WASM_TOOL])
    await go_to_extensions(page)

    card = page.locator(SEL["ext_card"]).first
    await card.wait_for(state="visible", timeout=5000)

    assert "Test WASM Tool" in await card.locator(SEL["ext_card_title"]).text_content()
    assert "WASM" in await card.locator(SEL["ext_card_kind"]).first.text_content()
    assert await card.locator(SEL["ext_active_dot"]).count() == 1
    assert (
        await card.get_by_role("button", name="Remove Test WASM Tool").count() == 1
    )
    assert await card.locator(SEL["ext_tags"]).count() == 1


async def test_mcp_active_and_inactive_actions(page):
    """Active MCP offers Disable; inactive MCP offers Activate."""
    await mock_ext_apis(page, installed=[_MCP_ACTIVE, _MCP_INACTIVE])
    await go_to_extensions(page)

    active = page.locator(SEL["ext_card"], has_text="Test MCP Server")
    inactive = page.locator(SEL["ext_card"], has_text="Inactive MCP")
    await active.wait_for(state="visible", timeout=5000)

    assert await active.get_by_role("button", name="Disable").count() == 1
    assert await active.get_by_role("button", name="Activate").count() == 0
    assert await inactive.get_by_role("button", name="Activate").count() == 1


# --- Group C: registry + install ------------------------------------------


async def test_registry_entry_and_install(page):
    """A registry entry renders with Install; clicking it posts and refreshes."""
    install_posts = []

    async def handle_install(route):
        install_posts.append(json.loads(route.request.post_data or "{}"))
        await _fulfil(route, {"success": True, "message": "Installed"})

    await mock_ext_apis(page, registry=[_REGISTRY_WASM])
    await page.route("**/api/extensions/install", handle_install)
    await go_to_extensions(page)

    reg = _registry_panel(page)
    row = reg.locator("tbody tr").first
    await row.wait_for(state="visible", timeout=5000)
    assert "Registry Tool" in await row.text_content()

    # Once installed, the list refetch should surface the new card.
    installed_after = {**_WASM_TOOL, "name": "registry-tool", "display_name": "Registry Tool"}
    await page.route(
        "**/api/extensions", lambda r: _fulfil(r, {"extensions": [installed_after]})
    )

    await reg.get_by_role("button", name="Install").first.click()

    await page.locator(SEL["ext_card"]).first.wait_for(state="visible", timeout=8000)
    assert len(install_posts) >= 1, "Install API was not called"


# --- Group D: configure inline panel --------------------------------------


async def _route_setup(page, name, secrets, save_posts=None):
    async def handle(route):
        if route.request.method == "GET":
            await _fulfil(route, {"name": name, "kind": "wasm_tool", "secrets": secrets})
        else:
            if save_posts is not None:
                save_posts.append(json.loads(route.request.post_data or "{}"))
            await _fulfil(route, {"success": True, "message": "Saved"})

    await page.route(f"**/api/extensions/{name}/setup", handle)


async def test_configure_panel_fields(page):
    """Configure opens the inline panel with the prompt and a provided tick."""
    secrets = [
        {"name": "api_key", "prompt": "Enter API key", "optional": False, "provided": False, "auto_generate": False},
        {"name": "token", "prompt": "API Token", "optional": False, "provided": True, "auto_generate": False},
    ]
    await mock_ext_apis(page, installed=[_WASM_TOOL])
    await _route_setup(page, "test-tool", secrets)
    await go_to_extensions(page)

    card = page.locator(SEL["ext_card"]).first
    await card.get_by_role("button", name="Configure").click()

    panel = page.locator(SEL["ext_configure_panel"])
    await panel.wait_for(state="visible", timeout=5000)
    text = await panel.text_content()
    assert "Enter API key" in text and "API Token" in text
    assert await panel.locator(SEL["ext_configure_input"]).count() == 2
    # The provided field renders a stored-value tick.
    assert await panel.locator(SEL["ext_configure_provided_icon"]).count() == 1


async def test_configure_panel_empty(page):
    """A setup with no secrets shows the 'No setup needed' notice."""
    await mock_ext_apis(page, installed=[_WASM_TOOL])
    await _route_setup(page, "test-tool", [])
    await go_to_extensions(page)

    await page.locator(SEL["ext_card"]).first.get_by_role(
        "button", name="Configure"
    ).click()
    panel = page.locator(SEL["ext_configure_panel"])
    await panel.wait_for(state="visible", timeout=5000)
    assert "No setup needed" in await panel.locator(SEL["ext_configure_empty"]).text_content()


async def test_configure_save_posts(page):
    """Filling a field and clicking Save posts the setup values."""
    save_posts = []
    secrets = [
        {"name": "token", "prompt": "Token", "optional": False, "provided": False, "auto_generate": False},
    ]
    await mock_ext_apis(page, installed=[_WASM_TOOL])
    await _route_setup(page, "test-tool", secrets, save_posts=save_posts)
    await go_to_extensions(page)

    await page.locator(SEL["ext_card"]).first.get_by_role(
        "button", name="Configure"
    ).click()
    panel = page.locator(SEL["ext_configure_panel"])
    await panel.wait_for(state="visible", timeout=5000)
    await panel.locator(SEL["ext_configure_input"]).first.fill("mytoken123")
    await panel.get_by_role("button", name="Save").click()

    await page.wait_for_function("() => true", timeout=100)
    # Give the mutation a moment to fire.
    for _ in range(20):
        if save_posts:
            break
        await page.wait_for_timeout(100)
    assert len(save_posts) >= 1, "Setup save was not posted"
    assert save_posts[0].get("secrets", {}).get("token") == "mytoken123"


async def test_configure_cancel_closes(page):
    """Cancel dismisses the inline configure panel."""
    await mock_ext_apis(page, installed=[_WASM_TOOL])
    await _route_setup(page, "test-tool", [{"name": "t", "prompt": "Token", "optional": False, "provided": False, "auto_generate": False}])
    await go_to_extensions(page)

    await page.locator(SEL["ext_card"]).first.get_by_role(
        "button", name="Configure"
    ).click()
    panel = page.locator(SEL["ext_configure_panel"])
    await panel.wait_for(state="visible", timeout=5000)
    await panel.get_by_role("button", name="Cancel").click()
    await panel.wait_for(state="hidden", timeout=3000)


# --- Group E: remove (Kobalte AlertDialog) --------------------------------


async def test_remove_confirmed(page):
    """Confirming the remove dialog posts /remove and drops the card."""
    remove_posts = []

    async def handle_remove(route):
        remove_posts.append(True)
        await _fulfil(route, {"success": True, "message": "Removed"})

    await mock_ext_apis(page, installed=[_WASM_TOOL])
    await page.route("**/api/extensions/test-tool/remove", handle_remove)
    await go_to_extensions(page)

    card = page.locator(SEL["ext_card"]).first
    await card.get_by_role("button", name="Remove Test WASM Tool").click()

    dialog = page.locator(SEL["ext_remove_dialog"])
    await dialog.wait_for(state="visible", timeout=5000)

    # After removal the list is empty.
    await page.route(
        "**/api/extensions", lambda r: _fulfil(r, {"extensions": []})
    )
    await dialog.get_by_role("button", name="Remove extension").click()

    await page.wait_for_function(
        "() => document.querySelectorAll('.extensions-card').length === 0",
        timeout=8000,
    )
    assert len(remove_posts) >= 1, "Remove API was not called"


async def test_remove_cancelled_keeps_card(page):
    """Cancelling the remove dialog keeps the extension card."""
    await mock_ext_apis(page, installed=[_WASM_TOOL])
    await go_to_extensions(page)

    card = page.locator(SEL["ext_card"]).first
    await card.get_by_role("button", name="Remove Test WASM Tool").click()

    dialog = page.locator(SEL["ext_remove_dialog"])
    await dialog.wait_for(state="visible", timeout=5000)
    # The Kobalte AlertDialog.CloseButton carries a component-set accessible
    # name (not "Cancel"), so target it by its danger-ghost class instead.
    await dialog.locator(".dashboard-detail__ghost--danger").click()
    await dialog.wait_for(state="hidden", timeout=3000)
    assert await page.locator(SEL["ext_card"]).count() >= 1


# --- Group F: activate ----------------------------------------------------


async def test_activate_mcp_posts(page):
    """Clicking Activate on an inactive MCP posts to the activate endpoint."""
    await mock_ext_apis(page, installed=[_MCP_INACTIVE])
    await go_to_extensions(page)

    card = page.locator(SEL["ext_card"]).first
    await card.wait_for(state="visible", timeout=5000)
    async with page.expect_request(
        "**/api/extensions/test-mcp-inactive/activate", timeout=5000
    ):
        await card.get_by_role("button", name="Activate").click()


# --- Group G: WASM channel stepper ----------------------------------------


async def _load_channel(page, activation_status):
    ext = {**_WASM_CHANNEL, "activation_status": activation_status}
    await mock_ext_apis(page, installed=[ext])
    await go_to_extensions(page)
    card = page.locator(SEL["ext_card"]).first
    await card.wait_for(state="visible", timeout=5000)
    return card


async def test_wasm_channel_stepper_installed(page):
    """An installed channel shows a 3-step stepper."""
    card = await _load_channel(page, "installed")
    stepper = card.locator(SEL["ext_stepper"])
    assert await stepper.count() == 1
    assert await stepper.locator(SEL["ext_stepper_step"]).count() == 3


async def test_wasm_channel_stepper_active(page):
    """An active channel marks every step completed (✓)."""
    card = await _load_channel(page, "active")
    circles = card.locator(SEL["ext_stepper"]).locator(SEL["ext_stepper_circle"])
    count = await circles.count()
    texts = [await circles.nth(i).text_content() for i in range(count)]
    assert all("✓" in (t or "") for t in texts), f"Expected all ✓: {texts}"


async def test_wasm_channel_stepper_failed(page):
    """A failed channel renders a ✗ in the stepper."""
    card = await _load_channel(page, "failed")
    circles = card.locator(SEL["ext_stepper"]).locator(SEL["ext_stepper_circle"])
    count = await circles.count()
    texts = [await circles.nth(i).text_content() for i in range(count)]
    assert any("✗" in (t or "") for t in texts), f"Expected a ✗: {texts}"


# --- Group H: pairing -----------------------------------------------------


async def test_pairing_list_and_approve(page):
    """A channel with pending pairing requests lists them and approves."""
    approve_posts = []

    async def handle_pairing(route):
        url = route.request.url
        if url.rstrip("/").endswith("/approve"):
            approve_posts.append(json.loads(route.request.post_data or "{}"))
            await _fulfil(route, {"success": True, "message": "Approved"})
        else:
            await _fulfil(
                route,
                {
                    "channel": "test-channel",
                    "requests": [
                        {"code": "ABC123", "sender_id": "peer-1", "created_at": "2026-01-01T00:00:00Z"}
                    ],
                },
            )

    await mock_ext_apis(page, installed=[_WASM_CHANNEL])
    # Registered after the default pairing route, so this wins (LIFO).
    await page.route("**/api/pairing/**", handle_pairing)
    await go_to_extensions(page)

    pairing = page.locator(SEL["ext_pairing"])
    await pairing.wait_for(state="visible", timeout=5000)
    assert "ABC123" in await pairing.locator(SEL["ext_pairing_code"]).text_content()

    await pairing.get_by_role("button", name="Approve pairing ABC123").click()
    for _ in range(20):
        if approve_posts:
            break
        await page.wait_for_timeout(100)
    assert len(approve_posts) >= 1, "Approve API was not called"
    assert approve_posts[0].get("code") == "ABC123"
