"""Tests for the ``page`` fixture's readiness gate in conftest.py.

The fixture must not hand a page to a test before the event stream is open:
the "Connected" label is in the static HTML, so only the stream itself says
the server's responses will arrive. These tests drive the fixture against
recording stand-ins, so the ordering is checked without a server or browser.
"""

import conftest
import pytest


class _Page:
    """Records the calls the fixture makes, in order, and can fail the wait."""

    def __init__(self, calls: list[str], fail_readiness: bool = False) -> None:
        self._calls = calls
        self._fail_readiness = fail_readiness

    async def goto(self, url: str) -> None:
        self._calls.append("goto")

    async def wait_for_selector(self, selector: str, **kwargs: object) -> None:
        self._calls.append(f"selector:{selector}")

    async def wait_for_function(self, expression: str, **kwargs: object) -> None:
        self._calls.append(f"function:{expression}")
        if self._fail_readiness:
            raise TimeoutError("the stream never opened")


class _Context:
    def __init__(self, calls: list[str], fail_readiness: bool) -> None:
        self._calls = calls
        self._page = _Page(calls, fail_readiness)

    async def new_page(self) -> _Page:
        return self._page

    async def close(self) -> None:
        self._calls.append("close")


class _Browser:
    def __init__(self, calls: list[str], fail_readiness: bool = False) -> None:
        self._calls = calls
        self._fail_readiness = fail_readiness

    async def new_context(self, **kwargs: object) -> _Context:
        return _Context(self._calls, self._fail_readiness)


def _fixture_body():
    """Return the undecorated ``page`` fixture function."""
    return conftest.page.__wrapped__


async def test_the_page_is_not_yielded_before_the_event_stream_is_open() -> None:
    calls: list[str] = []
    gen = _fixture_body()("http://127.0.0.1:1", _Browser(calls))
    await gen.__anext__()  # runs up to the yield
    assert any(
        call.startswith("function:") and "EventSource.OPEN" in call for call in calls
    ), f"the fixture yielded without waiting for the stream to open: {calls}"
    assert calls.index("selector:#auth-screen") < next(
        index for index, call in enumerate(calls) if call.startswith("function:")
    ), "the stream wait must follow the auth-screen wait"
    with pytest.raises(StopAsyncIteration):
        await gen.__anext__()
    assert calls[-1] == "close"


async def test_a_failed_readiness_wait_closes_the_context_and_never_yields() -> None:
    calls: list[str] = []
    gen = _fixture_body()("http://127.0.0.1:1", _Browser(calls, fail_readiness=True))
    with pytest.raises(TimeoutError):
        await gen.__anext__()
    assert calls[-1] == "close", f"the browser context leaked: {calls}"
