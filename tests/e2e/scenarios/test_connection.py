"""Scenario 1: Connection, auth, and route navigation (SolidJS shell).

Adaptation from the legacy shell:
  - The tab bar (`.tab-bar button`) is replaced by the shell's role=navigation
    links; "showing a panel" is now a route change with a per-route landmark.
  - The connection indicator is `[data-testid="sse-status"]` with a
    `data-state` attribute rather than textContent; it only reaches
    `connected` once the chat stream opens (visiting /chat).
"""

from helpers import AUTH_TOKEN, ROUTES, SEL, goto_route


async def test_page_loads_and_connects(page):
    """With a token the shell mounts, nav renders, and the chat stream connects."""
    # The app shell nav is visible (already awaited by the fixture).
    nav = page.get_by_role("navigation")
    assert await nav.is_visible()

    # Every route link is present and named.
    for name, _path in ROUTES:
        link = page.get_by_role("link", name=name, exact=True)
        assert await link.count() >= 1, f"Nav link '{name}' missing"

    # The chat route opens the SSE stream; the indicator reaches `connected`.
    # The fixture lands on /chat (index redirects there), so the stream is open.
    status = page.locator(SEL["sse_status"])
    await status.wait_for(state="visible", timeout=10000)
    await page.locator(f"{SEL['sse_status']}[data-state='connected']").wait_for(
        state="visible", timeout=15000
    )


async def test_route_navigation(page):
    """Clicking each nav link changes the URL and renders its route landmark."""
    for name, path in ROUTES:
        await goto_route(page, name, path, timeout=8000)
        assert page.url.rstrip("/").endswith(f"/{path}"), (
            f"URL did not change to /{path}: {page.url}"
        )

    # Return to Chat and confirm the composer is back.
    await goto_route(page, "Chat", "chat", timeout=8000)
    composer = page.get_by_label("Message composer")
    await composer.wait_for(state="visible", timeout=5000)


async def test_auth_rejection(page, axinite_server):
    """Navigating without a token shows the auth screen."""
    # A fresh context has empty sessionStorage, so no stored token leaks in
    # (the token is persisted per-context by AuthGate, see auth/token.ts).
    context = await page.context.browser.new_context()
    new_page = await context.new_page()
    try:
        await new_page.goto(axinite_server)
        auth_screen = new_page.locator(SEL["auth_screen"])
        await auth_screen.wait_for(state="visible", timeout=10000)
    finally:
        await context.close()
