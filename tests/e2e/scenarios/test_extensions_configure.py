"""Scenario: Extensions route — configure panel and removal.

See ``extensions_support`` for the fixture data, the API interception, and
the notes on how this coverage was adapted from the legacy shell.
"""

import json

from extensions_support import (
    fulfil,
    go_to_extensions,
    mock_ext_apis,
    WASM_TOOL,
)
from helpers import SEL, wait_until


# --- Group D: configure inline panel --------------------------------------


async def _route_setup(page, name, secrets, save_posts=None):
    async def handle(route):
        if route.request.method == "GET":
            await fulfil(route, {"name": name, "kind": "wasm_tool", "secrets": secrets})
        else:
            if save_posts is not None:
                save_posts.append(json.loads(route.request.post_data or "{}"))
            await fulfil(route, {"success": True, "message": "Saved"})

    await page.route(f"**/api/extensions/{name}/setup", handle)


async def test_configure_panel_fields(page):
    """Configure opens the inline panel with the prompt and a provided tick."""
    secrets = [
        {"name": "api_key", "prompt": "Enter API key", "optional": False, "provided": False, "auto_generate": False},
        {"name": "token", "prompt": "API Token", "optional": False, "provided": True, "auto_generate": False},
    ]
    await mock_ext_apis(page, installed=[WASM_TOOL])
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
    await mock_ext_apis(page, installed=[WASM_TOOL])
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
    await mock_ext_apis(page, installed=[WASM_TOOL])
    await _route_setup(page, "test-tool", secrets, save_posts=save_posts)
    await go_to_extensions(page)

    await page.locator(SEL["ext_card"]).first.get_by_role(
        "button", name="Configure"
    ).click()
    panel = page.locator(SEL["ext_configure_panel"])
    await panel.wait_for(state="visible", timeout=5000)
    await panel.locator(SEL["ext_configure_input"]).first.fill("mytoken123")
    await panel.get_by_role("button", name="Save").click()

    # Give the mutation a moment to fire.
    await wait_until(lambda: save_posts)
    assert len(save_posts) >= 1, "Setup save was not posted"
    assert save_posts[0].get("secrets", {}).get("token") == "mytoken123"


async def test_configure_cancel_closes(page):
    """Cancel dismisses the inline configure panel."""
    await mock_ext_apis(page, installed=[WASM_TOOL])
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
        await fulfil(route, {"success": True, "message": "Removed"})

    await mock_ext_apis(page, installed=[WASM_TOOL])
    await page.route("**/api/extensions/test-tool/remove", handle_remove)
    await go_to_extensions(page)

    card = page.locator(SEL["ext_card"]).first
    await card.get_by_role("button", name="Remove Test WASM Tool").click()

    dialog = page.locator(SEL["ext_remove_dialog"])
    await dialog.wait_for(state="visible", timeout=5000)

    # After removal the list is empty.
    await page.route(
        "**/api/extensions", lambda r: fulfil(r, {"extensions": []})
    )
    await dialog.get_by_role("button", name="Remove extension").click()

    await page.wait_for_function(
        "() => document.querySelectorAll('.extensions-card').length === 0",
        timeout=8000,
    )
    assert len(remove_posts) >= 1, "Remove API was not called"


async def test_remove_cancelled_keeps_card(page):
    """Cancelling the remove dialog keeps the extension card."""
    await mock_ext_apis(page, installed=[WASM_TOOL])
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
