"""Tests for the ``page`` fixture's readiness gate in conftest.py.

The fixture must not hand a page to a test before the event stream is open.
The SolidJS connection indicator exposes its actual stream state. These
tests drive the fixture against recording stand-ins, so the ordering is
checked without a server or browser.
"""

import conftest
import pytest
from helpers import SEL

CONNECTED_SELECTOR = f'{SEL["sse_status"]}[data-state="connected"]'


class _Locator:
    """Record a role wait without needing a real browser."""

    def __init__(self, page: "_Page", role: str) -> None:
        """Store the page and the role that the locator waits on."""
        self._page = page
        self._role = role

    async def wait_for(self, **kwargs: object) -> None:
        """Record the role wait on the owning page and ignore its options."""
        self._page.record_wait(f"role:{self._role}")


class _Page:
    """Records the calls the fixture makes, in order, and can fail the wait."""

    def __init__(self, calls: list[str], fail_at: str | None = None) -> None:
        """Store the shared call log and the wait that should fail, if any."""
        self._calls = calls
        self._fail_at = fail_at

    async def goto(self, url: str) -> None:
        """Record the navigation without loading the URL."""
        self._calls.append("goto")

    async def wait_for_selector(self, selector: str, **kwargs: object) -> None:
        """Record a wait for the selector."""
        self.record_wait(f"selector:{selector}")

    def get_by_role(self, role: str) -> _Locator:
        """Return a locator that records waits for the role."""
        return _Locator(self, role)

    def record_wait(self, call: str) -> None:
        """Log the wait, and raise ``TimeoutError`` if it is the one to fail."""
        self._calls.append(call)
        if call == self._fail_at:
            raise TimeoutError("the page never became ready")


class _Context:
    """Stand in for a browser context that hands out one recording page."""

    def __init__(self, calls: list[str], fail_at: str | None) -> None:
        """Create the recording page that the context hands out."""
        self._calls = calls
        self._page = _Page(calls, fail_at)

    async def new_page(self) -> _Page:
        """Return the recording page for this context."""
        return self._page

    async def close(self) -> None:
        """Record that the context was closed."""
        self._calls.append("close")


class _Browser:
    """Stand in for a browser that opens recording contexts."""

    def __init__(self, calls: list[str], fail_at: str | None = None) -> None:
        """Store the shared call log and the wait that should fail, if any."""
        self._calls = calls
        self._fail_at = fail_at

    async def new_context(self, **kwargs: object) -> _Context:
        """Return a recording context, ignoring the context options."""
        return _Context(self._calls, self._fail_at)


def _fixture_body():
    """Return the undecorated ``page`` fixture function."""
    return conftest.page.__wrapped__


async def test_the_page_is_not_yielded_before_the_event_stream_is_open() -> None:
    """Check that the page is yielded only after the event stream connects.

    Teardown then closes the context and makes no further browser calls.
    """
    calls: list[str] = []
    gen = _fixture_body()("http://127.0.0.1:1", _Browser(calls))
    await gen.__anext__()  # runs up to the yield
    ready_calls = [
        "goto",
        f'selector:{SEL["auth_screen"]}',
        "role:navigation",
        f"selector:{CONNECTED_SELECTOR}",
    ]
    assert calls == ready_calls, "the page must authenticate, mount, and connect"
    with pytest.raises(StopAsyncIteration):
        await gen.__anext__()
    assert calls == [*ready_calls, "close"], "no browser calls may follow cleanup"


@pytest.mark.parametrize(
    "fail_at",
    [
        f'selector:{SEL["auth_screen"]}',
        "role:navigation",
        f"selector:{CONNECTED_SELECTOR}",
    ],
    ids=["authentication", "navigation", "stream"],
)
async def test_a_failed_readiness_wait_closes_the_context_and_never_yields(
    fail_at: str,
) -> None:
    """Check that a failed readiness wait closes the context without yielding."""
    calls: list[str] = []
    gen = _fixture_body()("http://127.0.0.1:1", _Browser(calls, fail_at=fail_at))
    with pytest.raises(TimeoutError, match="the page never became ready"):
        await gen.__anext__()
    assert calls[-1] == "close", f"the browser context leaked: {calls}"
