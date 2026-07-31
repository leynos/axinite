"""Scenario 2: Chat round-trip against the mock LLM (SolidJS chat surface).

Adaptation from the legacy shell:
  - Messages are sent by filling the "Message composer" textarea and clicking
    the Send button (the SolidJS textarea has no Enter-to-send handler; Send is
    disabled while the composer is empty).
  - Turns are `[data-role="user"]` / `[data-role="assistant"]` wrappers; the
    assistant body is rendered markdown in `.chat-preview__markdown`.

This file also carries the auth-card coverage: in the SolidJS UI the extension
auth cards moved from the extensions surface into chat (chat-cards.tsx). They
are driven by the `auth_required` / `auth_completed` chat events, injected here
through the sanctioned `window.__axinite.emitChatEvent` hook.
"""

import json

from helpers import SEL


async def _send(page, text: str):
    composer = page.get_by_label("Message composer")  # aria-label, en-GB default
    await composer.wait_for(state="visible", timeout=5000)
    await composer.fill(text)
    send = page.locator(SEL["chat_send"])
    await send.click()


async def _assistant_markdown_contains(page, needle: str, *, timeout: int = 60000):
    # Generous timeout: the daemon's first LLM round-trip after startup is cold
    # (thread setup, pipeline warmup) and can take tens of seconds.
    # Inline the needle (JSON-escaped) rather than passing `arg=`; the latter is
    # unreliable with this Playwright build.
    needle_js = json.dumps(needle)
    try:
        await page.wait_for_function(
            f"""() => [...document.querySelectorAll(
                "[data-role='assistant'] .chat-preview__markdown"
            )].some((el) => (el.textContent || '').includes({needle_js}))""",
            timeout=timeout,
        )
    except Exception:
        texts = await page.eval_on_selector_all(
            "[data-role='assistant'] .chat-preview__markdown",
            "els => els.map(e => e.textContent)",
        )
        raise AssertionError(
            f"No assistant markdown contained {needle!r}. Seen: {texts!r}"
        )


async def test_send_message_and_receive_response(page):
    """Type a message, receive the mock LLM's canned '4' answer."""
    await _send(page, "What is 2+2?")

    # Assistant markdown eventually contains "4" (mock: "The answer is 4.").
    await _assistant_markdown_contains(page, "4")

    # The user message is echoed in a user turn.
    user_turns = page.locator(SEL["chat_turn_user"])
    assert await user_turns.count() >= 1
    user_text = await user_turns.last.text_content()
    assert "2+2" in user_text or "2 + 2" in user_text


async def test_multiple_messages(page):
    """Two messages produce two persisted assistant answers."""
    await _send(page, "Hello")
    await _assistant_markdown_contains(page, "Hello")  # mock greets back

    await _send(page, "What is 2+2?")
    await _assistant_markdown_contains(page, "4")

    # At least two of each persisted turn once streaming settles.
    await page.wait_for_function(
        """() => document.querySelectorAll(
            "[data-role='user']"
        ).length >= 2 && document.querySelectorAll(
            "[data-role='assistant'] .chat-preview__markdown"
        ).length >= 2""",
        timeout=15000,
    )


async def test_empty_message_not_sent(page):
    """Send is disabled for empty input, so no turn is created."""
    composer = page.get_by_label("Message composer")
    await composer.wait_for(state="visible", timeout=5000)

    send = page.locator(SEL["chat_send"])
    assert await send.is_disabled(), "Send should be disabled with an empty composer"

    initial = await page.locator(
        f"{SEL['chat_turn_user']}, {SEL['chat_turn_assistant']}"
    ).count()

    # Force a click attempt anyway; a disabled button dispatches nothing.
    await send.click(force=True)
    await page.wait_for_timeout(1000)

    final = await page.locator(
        f"{SEL['chat_turn_user']}, {SEL['chat_turn_assistant']}"
    ).count()
    assert final == initial, "Empty submit must not add a turn"


# --- Auth cards (chat-cards.tsx, driven by auth_required/auth_completed) ---


async def _emit(page, event: dict):
    await page.evaluate(
        "(event) => window.__axinite.emitChatEvent(event)", event
    )


async def test_auth_card_token_only(page):
    """auth_required without auth_url shows a token-only card (no OAuth button)."""
    await _emit(
        page,
        {
            "type": "auth_required",
            "extension_name": "github",
            "instructions": "Paste your GitHub token",
        },
    )
    card = page.locator(SEL["auth_card"])
    await card.wait_for(state="visible", timeout=5000)

    assert (
        await card.locator(SEL["auth_card_title"]).text_content()
        == "Authentication required for github"
    )
    assert "Paste your GitHub token" in await card.locator(
        SEL["auth_card_instructions"]
    ).text_content()
    assert await card.locator(SEL["auth_card_input"]).count() == 1
    assert await card.locator(SEL["auth_card_submit"]).count() == 1
    assert await card.locator(SEL["auth_card_cancel"]).count() == 1
    assert await card.locator(SEL["auth_card_oauth"]).count() == 0


async def test_auth_card_with_oauth(page):
    """auth_required with a http(s) auth_url renders the OAuth button."""
    await _emit(
        page,
        {
            "type": "auth_required",
            "extension_name": "slack",
            "auth_url": "https://slack.com/oauth/authorize",
        },
    )
    card = page.locator(SEL["auth_card"])
    await card.wait_for(state="visible", timeout=5000)
    assert await card.locator(SEL["auth_card_oauth"]).count() == 1


async def test_auth_card_submit_success_removes_card(page):
    """Submitting a token posts to /api/chat/auth-token and clears the card."""
    posts = []

    async def handle(route):
        posts.append(json.loads(route.request.post_data or "{}"))
        await route.fulfill(
            status=200,
            content_type="application/json",
            body=json.dumps({"success": True, "message": "Authenticated"}),
        )

    await page.route("**/api/chat/auth-token", handle)
    await _emit(page, {"type": "auth_required", "extension_name": "myext"})

    card = page.locator(SEL["auth_card"])
    await card.wait_for(state="visible", timeout=5000)
    await card.locator(SEL["auth_card_input"]).fill("valid-token-123")
    await card.locator(SEL["auth_card_submit"]).click()

    await card.wait_for(state="hidden", timeout=5000)
    assert len(posts) >= 1 and posts[0].get("token") == "valid-token-123"


async def test_auth_completed_dismisses_card(page):
    """An auth_completed event removes the card and shows a notice."""
    await _emit(page, {"type": "auth_required", "extension_name": "myext"})
    await page.locator(SEL["auth_card"]).wait_for(state="visible", timeout=5000)

    await _emit(
        page,
        {
            "type": "auth_completed",
            "extension_name": "myext",
            "success": True,
            "message": "All set",
        },
    )
    await page.locator(SEL["auth_card"]).wait_for(state="hidden", timeout=5000)
    assert "All set" in await page.locator(SEL["auth_notice"]).text_content()
