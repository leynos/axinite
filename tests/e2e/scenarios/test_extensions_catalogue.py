"""Scenario: Extensions route — installed cards, tools, registry, activation.

See ``extensions_support`` for the fixture data, the API interception, and
the notes on how this coverage was adapted from the legacy shell.
"""

import json

from extensions_support import (
    fulfil,
    go_to_extensions,
    MCP_ACTIVE,
    MCP_INACTIVE,
    mcp_panel,
    mock_ext_apis,
    registry_panel,
    REGISTRY_WASM,
    SAMPLE_TOOLS,
    tools_tbody,
    WASM_TOOL,
)
from helpers import SEL


# --- Group A: structural / empty state ------------------------------------


async def test_extensions_empty_state(page):
    """No installed cards; MCP panel empty message; empty tools table."""
    await mock_ext_apis(page)
    await go_to_extensions(page)

    assert await page.locator(SEL["ext_card"]).count() == 0
    mcp = mcp_panel(page)
    assert "No MCP servers available" in await mcp.text_content()
    assert await tools_tbody(page).locator("tr").count() == 0


async def test_tools_table_populated(page):
    """Two mock tools produce two rows in the tools table."""
    await mock_ext_apis(page, tools=SAMPLE_TOOLS)
    await go_to_extensions(page)

    tbody = tools_tbody(page)
    await tbody.locator("tr").first.wait_for(state="visible", timeout=5000)
    assert await tbody.locator("tr").count() == 2
    text = await tbody.text_content()
    assert "echo" in text and "time" in text

# --- Group B: installed cards ---------------------------------------------


async def test_installed_wasm_tool_card(page):
    """An active WASM tool card shows name, kind pill, active dot, remove."""
    await mock_ext_apis(page, installed=[WASM_TOOL])
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
    await mock_ext_apis(page, installed=[MCP_ACTIVE, MCP_INACTIVE])
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
        await fulfil(route, {"success": True, "message": "Installed"})

    await mock_ext_apis(page, registry=[REGISTRY_WASM])
    await page.route("**/api/extensions/install", handle_install)
    await go_to_extensions(page)

    reg = registry_panel(page)
    row = reg.locator("tbody tr").first
    await row.wait_for(state="visible", timeout=5000)
    assert "Registry Tool" in await row.text_content()

    # Once installed, the list refetch should surface the new card.
    installed_after = {**WASM_TOOL, "name": "registry-tool", "display_name": "Registry Tool"}
    await page.route(
        "**/api/extensions", lambda r: fulfil(r, {"extensions": [installed_after]})
    )

    await reg.get_by_role("button", name="Install").first.click()

    await page.locator(SEL["ext_card"]).first.wait_for(state="visible", timeout=8000)
    assert len(install_posts) >= 1, "Install API was not called"

# --- Group F: activate ----------------------------------------------------


async def test_activate_mcp_posts(page):
    """Clicking Activate on an inactive MCP posts to the activate endpoint."""
    await mock_ext_apis(page, installed=[MCP_INACTIVE])
    await go_to_extensions(page)

    card = page.locator(SEL["ext_card"]).first
    await card.wait_for(state="visible", timeout=5000)
    async with page.expect_request(
        "**/api/extensions/test-mcp-inactive/activate", timeout=5000
    ):
        await card.get_by_role("button", name="Activate").click()
