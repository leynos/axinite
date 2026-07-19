"""Scenario 5: HTML/XSS defence in chat messages (SolidJS pipeline).

Adaptation from the legacy shell:
  - The legacy `addMessage('assistant', ...)` global no longer exists. The
    assistant XSS vector is exercised through the real pipeline: the mock LLM
    returns a canned payload full of `<script>/<iframe>/onerror` for a message
    matching "html/injection test", and the SolidJS sanitizing markdown
    renderer (`renderMarkdown`) must strip them.
  - User input is verified as escaped plain text in the user turn bubble.
"""

from helpers import SEL


async def _send(page, text: str):
    composer = page.get_by_label("Message composer")
    await composer.wait_for(state="visible", timeout=5000)
    await composer.fill(text)
    await page.locator(SEL["chat_send"]).click()


async def test_assistant_xss_sanitized(page):
    """The mock LLM's XSS payload is rendered without executable markup."""
    await _send(page, "Please run the html injection test")

    # Wait for the assistant answer to render (any non-empty markdown block).
    await page.wait_for_function(
        """() => [...document.querySelectorAll(
            "[data-role='assistant'] .chat-preview__markdown"
        )].some((el) => (el.textContent || '').trim().length > 0)""",
        timeout=20000,
    )

    # Collect every assistant markdown block and assert none carry live markup.
    blocks = page.locator(f"{SEL['chat_turn_assistant']} {SEL['chat_markdown']}")
    combined = ""
    for i in range(await blocks.count()):
        combined += (await blocks.nth(i).inner_html()).lower()

    assert "<script" not in combined, "Script tags were not sanitized"
    assert "<iframe" not in combined, "iframe tags were not sanitized"
    assert "onerror=" not in combined, "Event handler attributes were not sanitized"

    # No <script> elements should exist anywhere in the assistant turns.
    script_count = await page.locator(f"{SEL['chat_turn_assistant']} script").count()
    assert script_count == 0, f"Found {script_count} script elements in chat"


async def test_user_message_rendered_as_plain_text(page):
    """A user message with an <img onerror> payload is shown as escaped text."""
    dangerous = '<img src=x onerror="alert(1)">'
    await _send(page, dangerous)

    user_turn = page.locator(SEL["chat_turn_user"]).last
    await user_turn.wait_for(state="visible", timeout=5000)

    text = await user_turn.text_content()
    assert "<img" in text, "User HTML should be visible as literal text"

    inner = await user_turn.inner_html()
    assert "&lt;img" in inner, "User message must be HTML-escaped, not rendered"
    # The escaped text legitimately contains the literal substring "onerror="
    # (it is inside an escaped text node, not a live attribute). The real
    # security assertion is that no actual <img> element was created.
    assert await user_turn.locator("img").count() == 0, (
        "User payload must not create a live <img> element"
    )
