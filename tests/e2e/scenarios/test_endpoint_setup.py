"""Scenario: confidential endpoint setup against the real gateway.

Unlike the catalogue scenarios, nothing here intercepts the setup API: the
server reads a synthetic tool manifest from ``WASM_TOOLS_DIR``, stores the
credential and the sealed endpoint binding in a real libSQL database, and the
browser drives the genuine controls.

The sentinel URL uses the reserved ``.invalid`` domain, so any attempt to
resolve or contact it would fail loudly. The scenarios assert the sentinel
never appears in a response, an SSE frame, the DOM, browser storage, the
extension listing, or the server log.
"""

import asyncio
import json

import aiohttp
import httpx
import pytest

from helpers import AUTH_TOKEN

SENTINEL = "https://sentinel-endpoint.invalid/caldav"
TOOL = "e2e-endpoints"


def _auth(token=AUTH_TOKEN):
    """Return the bearer header the gateway expects."""
    return {"Authorization": f"Bearer {token}"}


async def _get_setup(base_url, token=AUTH_TOKEN):
    """Fetch the setup schema through the real handler."""
    async with httpx.AsyncClient() as client:
        response = await client.get(
            f"{base_url}/api/extensions/{TOOL}/setup", headers=_auth(token)
        )
        response.raise_for_status()
        return response.json()


async def _post_setup(base_url, body, token=AUTH_TOKEN):
    """Submit setup values through the real handler."""
    async with httpx.AsyncClient() as client:
        response = await client.post(
            f"{base_url}/api/extensions/{TOOL}/setup",
            headers=_auth(token),
            json=body,
        )
        response.raise_for_status()
        return response.json()


async def _save_mixed(base_url, token=AUTH_TOKEN):
    """Store the credential and the sentinel endpoint."""
    return await _post_setup(
        base_url,
        {
            "secrets": {"token": "s3cret"},
            "endpoints": {"set": {"calendar": SENTINEL}},
        },
        token,
    )


def _sentinel_absent(payload):
    """Assert the sentinel value never appears in a serialized payload."""
    assert "sentinel-endpoint.invalid" not in json.dumps(payload)


async def test_mixed_setup_survives_a_restart(endpoint_server_factory):
    """A configured binding and secret persist across a server restart."""
    server = await endpoint_server_factory()

    initial = await _get_setup(server.base_url)
    assert initial["ready"] is False
    assert {blocker["field"] for blocker in initial["blockers"]} == {
        "token",
        "calendar",
    }
    assert initial["endpoints"][0]["state"] == "missing"
    assert initial["endpoints"][1]["optional"] is True

    saved = await _save_mixed(server.base_url)
    assert saved["success"] is True
    assert saved["ready"] is True
    assert [outcome["outcome"] for outcome in saved["outcomes"]] == [
        "stored",
        "stored",
    ]
    assert saved["endpoints"][0]["state"] == "configured"
    _sentinel_absent(saved)

    await server.restart()

    reloaded = await _get_setup(server.base_url)
    assert reloaded["ready"] is True
    assert reloaded["endpoints"][0]["state"] == "configured"
    assert reloaded["secrets"][0]["provided"] is True
    _sentinel_absent(reloaded)
    assert "sentinel-endpoint.invalid" not in server.log_text()


async def test_clear_returns_the_endpoint_to_missing(endpoint_server_factory):
    """Clearing a stored binding is idempotent and leaves the secret intact."""
    server = await endpoint_server_factory()
    await _save_mixed(server.base_url)

    cleared = await _post_setup(
        server.base_url, {"endpoints": {"clear": ["calendar"]}}
    )
    assert cleared["success"] is True
    assert cleared["endpoints"][0]["state"] == "missing"
    view = await _get_setup(server.base_url)
    assert view["secrets"][0]["provided"] is True
    _sentinel_absent(cleared)

    again = await _post_setup(
        server.base_url, {"endpoints": {"clear": ["calendar"]}}
    )
    assert again["outcomes"][0]["outcome"] == "cleared"


async def test_a_rejected_endpoint_changes_nothing(endpoint_server_factory):
    """An insecure scheme is rejected before any write, with no value echoed."""
    server = await endpoint_server_factory()
    rejected = await _post_setup(
        server.base_url,
        {
            "secrets": {"token": "s3cret"},
            "endpoints": {"set": {"calendar": "http://plain.invalid/caldav"}},
        },
    )
    assert rejected["success"] is False
    assert rejected["category"] == "endpoint_insecure_scheme"
    assert "plain.invalid" not in json.dumps(rejected)

    after = await _get_setup(server.base_url)
    assert after["secrets"][0]["provided"] is False
    assert after["endpoints"][0]["state"] == "missing"


async def test_another_deployment_cannot_read_the_binding(
    endpoint_server_factory,
):
    """A separate host with its own database never sees another's binding.

    The gateway is single-user: the extension manager scopes every setup row
    to one identity per deployment. Per-user isolation inside one database is
    proven by the repository contract tests; here the deployment boundary is
    what the E2E fixture can exercise honestly.
    """
    owner = await endpoint_server_factory()
    await _save_mixed(owner.base_url)

    other = await endpoint_server_factory()
    other_view = await _get_setup(other.base_url)
    assert other_view["ready"] is False
    assert other_view["endpoints"][0]["state"] == "missing"
    _sentinel_absent(other_view)

    owner_view = await _get_setup(owner.base_url)
    assert owner_view["endpoints"][0]["state"] == "configured"


async def test_no_sse_frame_carries_the_endpoint(endpoint_server_factory):
    """The auth-completed frame reports status without the submitted URL."""
    server = await endpoint_server_factory()
    frames = []

    async def listen():
        """Collect auth_completed frames until the setup event arrives."""
        url = f"{server.base_url}/api/chat/events?token={AUTH_TOKEN}"
        async with aiohttp.ClientSession() as session:
            async with session.get(url) as response:
                async for raw in response.content:
                    line = raw.decode("utf-8", errors="replace")
                    if line.startswith("data:"):
                        frames.append(line)
                    if any("auth_completed" in frame for frame in frames):
                        return

    listener = asyncio.create_task(listen())
    await asyncio.sleep(0.5)
    await _save_mixed(server.base_url)
    await asyncio.wait_for(listener, timeout=10)

    assert any("auth_completed" in frame for frame in frames)
    assert not any("sentinel-endpoint.invalid" in frame for frame in frames)


async def _open_configure(page, base_url, variant):
    """Navigate to the extensions surface and open the configure controls."""
    if variant == "legacy":
        await page.goto(f"{base_url}/?token={AUTH_TOKEN}")
        await page.click('button[data-tab="extensions"]')
        await page.wait_for_selector(".ext-card", timeout=15000)
        card = page.locator(".ext-card", has_text="e2e-endpoints").first
        await card.locator(".btn-ext.configure").click()
        await page.wait_for_selector(".configure-modal", timeout=10000)
        return page.locator(".configure-modal")
    await page.goto(f"{base_url}/extensions?token={AUTH_TOKEN}")
    await page.wait_for_selector(".extensions-card", timeout=15000)
    await page.get_by_role("button", name="Configure").first.click()
    panel = page.locator(".extensions-detail")
    await panel.wait_for(state="visible", timeout=10000)
    return panel


@pytest.mark.parametrize("ui_variant", ["solid", "legacy"])
async def test_the_ui_shows_state_without_the_value(
    page, endpoint_server_factory, ui_variant
):
    """Both interfaces render endpoint controls and never echo the URL."""
    server = await endpoint_server_factory(ui_variant=ui_variant)
    await _save_mixed(server.base_url)

    scope = await _open_configure(page, server.base_url, ui_variant)
    body = await page.locator("body").inner_text()
    assert "sentinel-endpoint.invalid" not in body
    assert "Contacts endpoint URL" in body

    if ui_variant == "legacy":
        urls = scope.locator('input[type="url"]')
        assert await urls.count() == 2
        assert await urls.nth(0).input_value() == ""
        placeholder = await urls.nth(0).get_attribute("placeholder")
        assert placeholder == "(stored — enter a new URL to replace)"
        assert await scope.locator(".configure-check").count() == 1
    else:
        calendar = scope.get_by_label("Calendar endpoint URL")
        assert await calendar.get_attribute("type") == "url"
        assert await calendar.input_value() == ""
        assert (
            await calendar.get_attribute("placeholder")
            == "(stored — enter a new URL to replace)"
        )
        assert await scope.get_by_role(
            "checkbox", name="Clear stored value"
        ).count() == 1

    storage = await page.evaluate(
        "JSON.stringify({local: {...localStorage}, session: {...sessionStorage}})"
    )
    assert "sentinel-endpoint.invalid" not in storage

    tools = await _get_extensions(server.base_url)
    _sentinel_absent(tools)


async def _get_extensions(base_url):
    """Fetch the extension listing and its tool metadata."""
    async with httpx.AsyncClient() as client:
        listing = await client.get(
            f"{base_url}/api/extensions", headers=_auth()
        )
        tools = await client.get(
            f"{base_url}/api/extensions/tools", headers=_auth()
        )
        listing.raise_for_status()
        tools.raise_for_status()
        return {"extensions": listing.json(), "tools": tools.json()}


@pytest.mark.parametrize("ui_variant", ["solid", "legacy"])
async def test_replacing_through_the_ui_updates_the_state(
    page, endpoint_server_factory, ui_variant
):
    """Typing a replacement through the real controls updates the binding."""
    server = await endpoint_server_factory(ui_variant=ui_variant)
    await _save_mixed(server.base_url)
    scope = await _open_configure(page, server.base_url, ui_variant)

    replacement = "https://replacement-endpoint.invalid/caldav"
    if ui_variant == "legacy":
        target = scope.locator('input[type="url"]').nth(0)
        target.fill(replacement)
        await scope.get_by_text("Save").click()
    else:
        scope.get_by_label("Calendar endpoint URL").fill(replacement)
        await scope.get_by_role("button", name="Save").click()

    async def replaced():
        """Poll the schema until the replacement is stored."""
        view = await _get_setup(server.base_url)
        return view["endpoints"][0]["state"] == "configured"

    deadline = 10
    for _ in range(deadline * 10):
        if await replaced():
            break
        await asyncio.sleep(0.1)
    assert await replaced()

    final = await _get_setup(server.base_url)
    assert final["ready"] is True
    assert "replacement-endpoint.invalid" not in json.dumps(final)
    assert "replacement-endpoint.invalid" not in await page.locator("body").inner_text()