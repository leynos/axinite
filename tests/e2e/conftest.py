"""pytest fixtures for E2E tests.

Session-scoped: build binary, start mock LLM, start axinite, launch browser.
Function-scoped: fresh browser context and page per test.
"""

import asyncio
import os
import signal
import socket
import subprocess
import sys
import tempfile
from collections.abc import Generator
from pathlib import Path

import pytest

from helpers import AUTH_TOKEN, wait_for_port_line, wait_for_ready

# Project root (two levels up from tests/e2e/)
ROOT = Path(__file__).resolve().parent.parent.parent

# Temp directory for the libSQL database file (cleaned up automatically)
_DB_TMPDIR = tempfile.TemporaryDirectory(prefix="axinite-e2e-")

# The server's combined stdout and stderr. A file rather than a pipe: nothing
# reads a pipe after start-up, so a run chatty enough to fill it (64 KiB on
# Linux) would block the server's logging mid-test, and a failing test would
# have no server output to explain it.
_SERVER_LOG = Path(_DB_TMPDIR.name) / "axinite-server.log"
_SERVER_LOG_TAIL_BYTES = 16384


def _server_log_tail() -> str:
    """Return the end of the server log, with the auth token redacted.

    Reads at most ``_SERVER_LOG_TAIL_BYTES`` from the end of the file, so a
    chatty server does not make every failed test read the whole log. A log
    that cannot be read is reported as such rather than as an empty one: an
    empty tail would read as "the server said nothing".
    """
    try:
        with _SERVER_LOG.open("rb") as log:
            size = log.seek(0, os.SEEK_END)
            log.seek(max(0, size - _SERVER_LOG_TAIL_BYTES))
            data = log.read(_SERVER_LOG_TAIL_BYTES)
    except OSError as error:
        return f"(server log unavailable: {error})"
    return data.decode("utf-8", errors="replace").replace(AUTH_TOKEN, "***")


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(
    item: pytest.Item, call: pytest.CallInfo[None]
) -> Generator[None, object, None]:
    """Attach the server log tail to a report for any failed phase.

    A browser test that times out says only that something did not appear.
    The server's own log is what shows whether it received the request. The
    ``page`` fixture can fail in setup, so every phase is covered, not only
    the test call.
    """
    outcome = yield
    report = outcome.get_result()  # type: ignore[attr-defined]
    if report.failed:
        report.sections.append(("axinite server log (tail)", _server_log_tail()))


def _find_free_port() -> int:
    """Bind to port 0 and return the OS-assigned port."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


@pytest.fixture(scope="session")
def axinite_binary():
    """Ensure axinite binary is built. Returns the binary path."""
    binary = ROOT / "target" / "debug" / "axinite"
    if not binary.exists():
        print("Building axinite (this may take a while)...")
        subprocess.run(
            ["cargo", "build", "--no-default-features", "--features", "libsql"],
            cwd=ROOT,
            check=True,
            timeout=600,
        )
    assert binary.exists(), f"Binary not found at {binary}"
    return str(binary)


@pytest.fixture(scope="session")
async def mock_llm_server():
    """Start the mock LLM server. Yields the base URL."""
    server_script = Path(__file__).parent / "mock_llm.py"
    proc = await asyncio.create_subprocess_exec(
        sys.executable, str(server_script), "--port", "0",
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    try:
        port = await wait_for_port_line(proc, r"MOCK_LLM_PORT=(\d+)", timeout=10)
        url = f"http://127.0.0.1:{port}"
        await wait_for_ready(f"{url}/v1/models", timeout=10)
        yield url
    finally:
        proc.send_signal(signal.SIGTERM)
        try:
            await asyncio.wait_for(proc.wait(), timeout=5)
        except asyncio.TimeoutError:
            proc.kill()


@pytest.fixture(scope="session")
async def axinite_server(axinite_binary, mock_llm_server):
    """Start the axinite gateway. Yields the base URL."""
    gateway_port = _find_free_port()
    env = {
        # Minimal env: PATH for process spawning, HOME for Rust/cargo defaults
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", "/tmp"),
        "RUST_LOG": "axinite=info",
        "RUST_BACKTRACE": "1",
        "GATEWAY_ENABLED": "true",
        "GATEWAY_HOST": "127.0.0.1",
        "GATEWAY_PORT": str(gateway_port),
        "GATEWAY_AUTH_TOKEN": AUTH_TOKEN,
        "GATEWAY_USER_ID": "e2e-tester",
        "CLI_ENABLED": "false",
        "LLM_BACKEND": "openai_compatible",
        "LLM_BASE_URL": mock_llm_server,
        "LLM_MODEL": "mock-model",
        "DATABASE_BACKEND": "libsql",
        "LIBSQL_PATH": os.path.join(_DB_TMPDIR.name, "e2e.db"),
        "SANDBOX_ENABLED": "false",
        "SKILLS_ENABLED": "true",
        "ROUTINES_ENABLED": "false",
        "HEARTBEAT_ENABLED": "false",
        "EMBEDDING_ENABLED": "false",
        # Prevent onboarding wizard from triggering
        "ONBOARD_COMPLETED": "true",
    }
    # Forward LLVM coverage instrumentation env vars when present
    # (allows cargo-llvm-cov to collect profraw data from E2E runs).
    # Use prefix matching to stay resilient to cargo-llvm-cov changes.
    COV_ENV_PREFIXES = ("CARGO_LLVM_COV", "LLVM_")
    COV_ENV_EXTRAS = ("CARGO_ENCODED_RUSTFLAGS", "CARGO_INCREMENTAL")
    for key, val in os.environ.items():
        if key.startswith(COV_ENV_PREFIXES) or key in COV_ENV_EXTRAS:
            env[key] = val
    with _SERVER_LOG.open("wb") as server_log:
        proc = await asyncio.create_subprocess_exec(
            axinite_binary, "--no-onboard",
            stdin=asyncio.subprocess.DEVNULL,
            stdout=server_log,
            stderr=asyncio.subprocess.STDOUT,
            env=env,
        )
    base_url = f"http://127.0.0.1:{gateway_port}"
    try:
        await wait_for_ready(f"{base_url}/api/health", timeout=60)
        yield base_url
    except TimeoutError:
        # Dump the server log so CI logs show why the server failed to start
        returncode = proc.returncode
        stderr_text = _server_log_tail()
        proc.kill()
        pytest.fail(
            f"axinite server failed to start on port {gateway_port} "
            f"(returncode={returncode}).\nserver log:\n{stderr_text}"
        )
    finally:
        if proc.returncode is None:
            # Use SIGINT (not SIGTERM) so tokio's ctrl_c handler triggers a
            # graceful shutdown.  This lets the LLVM coverage runtime run its
            # atexit handler and flush .profraw files for cargo-llvm-cov.
            proc.send_signal(signal.SIGINT)
            try:
                await asyncio.wait_for(proc.wait(), timeout=10)
            except asyncio.TimeoutError:
                proc.kill()


@pytest.fixture(scope="session")
async def browser(axinite_server):
    """Session-scoped Playwright browser instance.

    Reuses a single browser process across all tests. Individual tests
    get isolated contexts via the ``page`` fixture.
    """
    from playwright.async_api import async_playwright

    headless = os.environ.get("HEADED", "").strip() not in ("1", "true")
    async with async_playwright() as p:
        b = await p.chromium.launch(headless=headless)
        yield b
        await b.close()


@pytest.fixture
async def page(axinite_server, browser):
    """Fresh Playwright browser context + page, navigated to the gateway with auth."""
    context = await browser.new_context(viewport={"width": 1280, "height": 720})
    try:
        pg = await context.new_page()
        await pg.goto(f"{axinite_server}/?token={AUTH_TOKEN}")
        # Wait for the app to initialize (auth screen hidden, SSE connected)
        await pg.wait_for_selector("#auth-screen", state="hidden", timeout=15000)
        # The status label reads "Connected" in the static HTML before the
        # stream opens, so it cannot gate the first message. A response
        # broadcast before the EventSource is open is not replayed to it, and
        # the test then waits for a message that was never delivered.
        # `eventSource` is a top-level `let` in app.js, so it is visible by
        # name but is not a `window` property.
        await pg.wait_for_function(
            "() => eventSource !== null"
            " && eventSource.readyState === EventSource.OPEN",
            timeout=15000,
        )
        yield pg
    finally:
        await context.close()
