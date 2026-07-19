"""Scenario 6: Tool approval card (SolidJS, history-driven).

Adaptation from the legacy shell:
  - There is no `showApproval()` global and no `.approval-card` overlay. The
    approval card is rendered from GET /api/chat/history's `pending_approval`
    field (a query). To inject one deterministically we intercept
    `/api/chat/history` and `/api/chat/threads` with `page.route`, then fire
    `window.__axinite.emitChatEvent({type:"approval_needed", ...})`, which the
    chat surface handles by refetching history (see chat-preview.tsx).
  - Approve/Always/Deny post to /api/chat/approval; the card clears once the
    refetched history no longer carries a pending approval. Legacy assertions
    on `.approval-resolved` ("Approved"/"Denied") no longer apply — the card
    simply disappears — so we assert the POST and the card removal instead.
"""

import json

from helpers import AUTH_TOKEN, SEL

_THREAD_ID = "e2e-approval-thread"

_APPROVAL = {
    "request_id": "test-req-001",
    "tool_name": "shell",
    "description": "Execute: echo hello world",
    "parameters": '{"command": "echo hello world"}',
}


async def _install_routes(page, phase, approval_posts):
    """Intercept threads/history/approval so the card is fully deterministic."""

    async def handle_threads(route):
        body = json.dumps(
            {
                "assistant_thread": {
                    "id": _THREAD_ID,
                    "state": "idle",
                    "turn_count": 0,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z",
                    "title": "Approval",
                },
                "threads": [],
                "active_thread": _THREAD_ID,
            }
        )
        await route.fulfill(status=200, content_type="application/json", body=body)

    async def handle_history(route):
        payload = {
            "thread_id": _THREAD_ID,
            "turns": [],
            "has_more": False,
        }
        if phase["value"] == "pending":
            payload["pending_approval"] = _APPROVAL
        await route.fulfill(
            status=200,
            content_type="application/json",
            body=json.dumps(payload),
        )

    async def handle_approval(route):
        approval_posts.append(json.loads(route.request.post_data or "{}"))
        # Once approved, subsequent history refetches carry no pending approval.
        phase["value"] = "approved"
        await route.fulfill(
            status=200,
            content_type="application/json",
            body=json.dumps({"success": True, "message": "Approved"}),
        )

    await page.route("**/api/chat/threads", handle_threads)
    await page.route("**/api/chat/history*", handle_history)
    await page.route("**/api/chat/approval", handle_approval)


async def _boot_chat(page, axinite_server, phase, approval_posts):
    await _install_routes(page, phase, approval_posts)
    # Reload so ChatPreview remounts and fetches threads/history through the
    # interception; the token persists in sessionStorage across the reload.
    await page.goto(f"{axinite_server}/chat")
    await page.wait_for_selector(SEL["auth_screen"], state="hidden", timeout=15000)
    await page.get_by_label("Message composer").wait_for(state="visible", timeout=10000)


async def _inject_approval(page):
    await page.evaluate(
        """() => window.__axinite.emitChatEvent({
            type: 'approval_needed',
            request_id: 'test-req-001',
            tool_name: 'shell',
            description: 'Execute: echo hello world',
            parameters: '{"command": "echo hello world"}'
        })"""
    )


async def test_approval_card_appears_with_fields(page, axinite_server):
    """Injecting a pending approval renders the card with its fields/actions."""
    phase = {"value": "none"}
    posts = []
    await _boot_chat(page, axinite_server, phase, posts)

    phase["value"] = "pending"
    await _inject_approval(page)

    card = page.locator(SEL["approval_card"])
    await card.wait_for(state="visible", timeout=8000)

    assert (await card.locator("h3").text_content()) == "shell"
    assert "echo hello world" in await card.text_content()

    # Localized en-GB action names are exactly Approve/Always/Deny.
    assert await card.get_by_role("button", name="Approve").count() == 1
    assert await card.get_by_role("button", name="Always").count() == 1
    assert await card.get_by_role("button", name="Deny").count() == 1


async def test_approve_posts_and_clears_card(page, axinite_server):
    """Approve posts to /api/chat/approval and the card clears on refetch."""
    phase = {"value": "none"}
    posts = []
    await _boot_chat(page, axinite_server, phase, posts)

    phase["value"] = "pending"
    await _inject_approval(page)

    card = page.locator(SEL["approval_card"])
    await card.wait_for(state="visible", timeout=8000)

    await card.get_by_role("button", name="Approve").click()

    # The POST was made with action=approve for our request id.
    await page.wait_for_function(
        "() => true", timeout=100
    )  # yield to the event loop
    await card.wait_for(state="hidden", timeout=8000)

    assert len(posts) >= 1, "Approval POST was not made"
    assert posts[0].get("action") == "approve"
    assert posts[0].get("request_id") == _APPROVAL["request_id"]
