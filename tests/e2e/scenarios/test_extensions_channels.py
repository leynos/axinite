"""Scenario: Extensions route — WASM channel stepper and pairing.

See ``extensions_support`` for the fixture data, the API interception, and
the notes on how this coverage was adapted from the legacy shell.
"""

import json

from extensions_support import (
    fulfil,
    go_to_extensions,
    mock_ext_apis,
    WASM_CHANNEL,
)
from helpers import SEL, wait_until


# --- Group G: WASM channel stepper ----------------------------------------


async def _load_channel(page, activation_status):
    ext = {**WASM_CHANNEL, "activation_status": activation_status}
    await mock_ext_apis(page, installed=[ext])
    await go_to_extensions(page)
    card = page.locator(SEL["ext_card"]).first
    await card.wait_for(state="visible", timeout=5000)
    return card


async def _stepper_circle_texts(card) -> list[str]:
    """Return the text of each step circle in a channel card's stepper."""
    circles = card.locator(SEL["ext_stepper"]).locator(SEL["ext_stepper_circle"])
    return await circles.all_text_contents()


async def test_wasm_channel_stepper_installed(page):
    """An installed channel shows a 3-step stepper."""
    card = await _load_channel(page, "installed")
    stepper = card.locator(SEL["ext_stepper"])
    assert await stepper.count() == 1
    assert await stepper.locator(SEL["ext_stepper_step"]).count() == 3


async def test_wasm_channel_stepper_active(page):
    """An active channel marks every step completed (✓)."""
    card = await _load_channel(page, "active")
    texts = await _stepper_circle_texts(card)
    assert all("✓" in t for t in texts), f"Expected all ✓: {texts}"


async def test_wasm_channel_stepper_failed(page):
    """A failed channel renders a ✗ in the stepper."""
    card = await _load_channel(page, "failed")
    texts = await _stepper_circle_texts(card)
    assert any("✗" in t for t in texts), f"Expected a ✗: {texts}"

# --- Group H: pairing -----------------------------------------------------


async def test_pairing_list_and_approve(page):
    """A channel with pending pairing requests lists them and approves."""
    approve_posts = []

    async def handle_pairing(route):
        url = route.request.url
        if url.rstrip("/").endswith("/approve"):
            approve_posts.append(json.loads(route.request.post_data or "{}"))
            await fulfil(route, {"success": True, "message": "Approved"})
        else:
            await fulfil(
                route,
                {
                    "channel": "test-channel",
                    "requests": [
                        {"code": "ABC123", "sender_id": "peer-1", "created_at": "2026-01-01T00:00:00Z"}
                    ],
                },
            )

    await mock_ext_apis(page, installed=[WASM_CHANNEL])
    # Registered after the default pairing route, so this wins (LIFO).
    await page.route("**/api/pairing/**", handle_pairing)
    await go_to_extensions(page)

    pairing = page.locator(SEL["ext_pairing"])
    await pairing.wait_for(state="visible", timeout=5000)
    assert "ABC123" in await pairing.locator(SEL["ext_pairing_code"]).text_content()

    await pairing.get_by_role("button", name="Approve pairing ABC123").click()
    await wait_until(lambda: approve_posts)
    assert len(approve_posts) >= 1, "Approve API was not called"
    assert approve_posts[0].get("code") == "ABC123"
