"""Scenario 3: Chat stream reconnection (SolidJS + window.__axinite hooks).

Adaptation from the legacy shell:
  - The legacy `eventSource`/`connectSSE()` globals are replaced by the
    sanctioned `window.__axinite` test hooks: `closeChatStream()` and
    `reconnectChatStream()`.
  - Connection state is read from `[data-testid="sse-status"]`'s `data-state`
    attribute (idle|connecting|connected|disconnected) rather than textContent.
"""

from helpers import SEL


async def _wait_state(page, state: str, *, timeout: int = 10000):
    await page.locator(f"{SEL['sse_status']}[data-state='{state}']").wait_for(
        state="visible", timeout=timeout
    )


async def test_sse_status_connected_on_chat(page):
    """Visiting /chat opens the stream and the indicator reaches `connected`."""
    await _wait_state(page, "connected", timeout=15000)


async def test_reconnect_cycle(page):
    """close -> disconnected, reconnect -> connected."""
    await _wait_state(page, "connected", timeout=15000)

    await page.evaluate("window.__axinite.closeChatStream()")
    await _wait_state(page, "disconnected")

    await page.evaluate("window.__axinite.reconnectChatStream()")
    await _wait_state(page, "connected", timeout=15000)


async def test_reconnect_preserves_history(page):
    """A message sent before a reconnect remains visible afterwards."""
    composer = page.get_by_label("Message composer")
    await composer.fill("Hello")
    await page.locator(SEL["chat_send"]).click()

    # Wait for the assistant answer so the turn is persisted in the DB.
    await page.wait_for_function(
        """() => [...document.querySelectorAll(
            "[data-role='assistant'] .chat-preview__markdown"
        )].some((el) => (el.textContent || '').length > 0)""",
        timeout=15000,
    )
    await _wait_state(page, "connected", timeout=15000)

    users_before = await page.locator(SEL["chat_turn_user"]).count()
    assert users_before >= 1

    # Cycle the stream via the test hooks.
    await page.evaluate("window.__axinite.closeChatStream()")
    await _wait_state(page, "disconnected")
    await page.evaluate("window.__axinite.reconnectChatStream()")
    await _wait_state(page, "connected", timeout=15000)

    # The history (query cache) survives the reconnect: the user turn is still
    # present.
    assert await page.locator(SEL["chat_turn_user"]).count() >= 1, (
        "User turn should remain visible after reconnect"
    )
