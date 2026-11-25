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
from dataclasses import dataclass
from pathlib import Path

import pytest

from helpers import AUTH_TOKEN, SEL, wait_for_port_line, wait_for_ready

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


# Deterministic master key for the endpoint-setup scenarios. Test-only.
ENDPOINT_TEST_MASTER_KEY = "fedcba9876543210fedcba9876543210"

# A synthetic tool manifest used by the endpoint-setup scenarios. It declares
# one credential and two endpoints, so the scenarios never depend on a shipped
# extension that does not exist yet.
ENDPOINT_TOOL_MANIFEST = """{
  "setup": {
    "fields": [
      {"type": "secret", "name": "token", "prompt": "API token"},
      {"type": "endpoint", "name": "calendar", "kind": "caldav",
       "prompt": "Calendar endpoint URL"},
      {"type": "endpoint", "name": "contacts", "kind": "carddav",
       "prompt": "Contacts endpoint URL", "optional": true}
    ]
  }
}"""


@dataclass(frozen=True)
class ServerSettings:
    """Everything one axinite test server needs beyond the shared mock LLM.

    Bundling these keeps ``_server_env`` readable and makes the identity and
    storage a server runs with explicit at every call site.
    """

    db_path: Path
    user_id: str
    auth_token: str
    home: Path
    tools_dir: Path | None = None
    ui_variant: str | None = None


def _server_env(mock_llm_url: str, settings: ServerSettings) -> dict[str, str]:
    """Build the deterministic environment for one axinite server process.

    ``settings.home`` must be private per concurrently running handle: the
    single-instance guard is a PID file under ``$HOME/.axinite``, and two live
    servers sharing one home would refuse to start.
    """
    db_path = settings.db_path
    user_id = settings.user_id
    auth_token = settings.auth_token
    home = settings.home
    tools_dir = settings.tools_dir
    ui_variant = settings.ui_variant
    env = {
        # Minimal env: PATH for process spawning, HOME for Rust/cargo defaults
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(home),
        "RUST_LOG": "axinite=info",
        "RUST_BACKTRACE": "1",
        "GATEWAY_ENABLED": "true",
        "GATEWAY_HOST": "127.0.0.1",
        "GATEWAY_AUTH_TOKEN": auth_token,
        "GATEWAY_USER_ID": user_id,
        "CLI_ENABLED": "false",
        "LLM_BACKEND": "openai_compatible",
        "LLM_BASE_URL": mock_llm_url,
        "LLM_MODEL": "mock-model",
        "DATABASE_BACKEND": "libsql",
        "LIBSQL_PATH": str(db_path),
        # A master key is required for the secrets store and the
        # endpoint-binding service; without it the setup route reports that
        # binding storage is unavailable.
        "SECRETS_MASTER_KEY": ENDPOINT_TEST_MASTER_KEY,
        "SANDBOX_ENABLED": "false",
        "SKILLS_ENABLED": "true",
        "ROUTINES_ENABLED": "false",
        "HEARTBEAT_ENABLED": "false",
        "EMBEDDING_ENABLED": "false",
        # Prevent onboarding wizard from triggering
        "ONBOARD_COMPLETED": "true",
        # SANDBOX_ENABLED/ROUTINES_ENABLED above leave the jobs and routines
        # runtimes unwired, so the subsystem-availability layer would hide
        # their routes. Force the route flags on (precedence layer 1) so the
        # scenarios can still walk every nav entry.
        "FEATURE_FLAG_ROUTE_JOBS": "true",
        "FEATURE_FLAG_ROUTE_ROUTINES": "true",
    }
    if tools_dir is not None:
        env["WASM_TOOLS_DIR"] = str(tools_dir)
    if ui_variant is not None:
        env["AXINITE_WEB_UI"] = ui_variant
    # Forward LLVM coverage instrumentation env vars when present
    # (allows cargo-llvm-cov to collect profraw data from E2E runs).
    # Use prefix matching to stay resilient to cargo-llvm-cov changes.
    COV_ENV_PREFIXES = ("CARGO_LLVM_COV", "LLVM_")
    COV_ENV_EXTRAS = ("CARGO_ENCODED_RUSTFLAGS", "CARGO_INCREMENTAL")
    for key, val in os.environ.items():
        if key.startswith(COV_ENV_PREFIXES) or key in COV_ENV_EXTRAS:
            env[key] = val
    return env


class AxiniteHandle:
    """A restartable axinite server process bound to one database."""

    def __init__(self, binary, env, log_path):
        """Remember the binary, the base environment, and the log path."""
        self._binary = binary
        self._env = dict(env)
        self._log_path = log_path
        self.proc = None
        self.base_url = None

    async def start(self):
        """Start the server on a free port and wait for its health probe."""
        port = _find_free_port()
        self._env["GATEWAY_PORT"] = str(port)
        with self._log_path.open("ab") as server_log:
            self.proc = await asyncio.create_subprocess_exec(
                self._binary,
                "--no-onboard",
                stdin=asyncio.subprocess.DEVNULL,
                stdout=server_log,
                stderr=asyncio.subprocess.STDOUT,
                env=self._env,
            )
        self.base_url = f"http://127.0.0.1:{port}"
        try:
            await wait_for_ready(f"{self.base_url}/api/health", timeout=60)
        except TimeoutError:
            tail = self.log_text()[-4000:]
            await self.stop()
            raise AssertionError(
                f"axinite did not become ready on {self.base_url}\n"
                f"server log tail:\n{tail}"
            ) from None

    async def stop(self):
        """Stop the server gracefully so libSQL releases the database file."""
        if self.proc is None or self.proc.returncode is not None:
            return
        self.proc.send_signal(signal.SIGINT)
        try:
            await asyncio.wait_for(self.proc.wait(), timeout=10)
        except asyncio.TimeoutError:
            self.proc.kill()

    async def restart(self):
        """Stop and start again against the same database and directories."""
        await self.stop()
        await self.start()

    def log_text(self):
        """Return everything the server has written to its log so far."""
        return self._log_path.read_text(encoding="utf-8", errors="replace")


@pytest.fixture
async def endpoint_server_factory(axinite_binary, mock_llm_server, tmp_path_factory):
    """Yield a builder for restartable servers that share one synthetic tool.

    The synthetic tool's manifest declares one credential and two endpoints,
    so the endpoint-setup scenarios exercise the real handlers and real
    libSQL storage without a route-interception mock.
    """
    root = tmp_path_factory.mktemp("endpoint-setup-e2e")
    tools_dir = root / "tools"
    tools_dir.mkdir()
    (tools_dir / "e2e-endpoints.wasm").write_bytes(b"")
    (tools_dir / "e2e-endpoints.capabilities.json").write_text(
        ENDPOINT_TOOL_MANIFEST, encoding="utf-8"
    )
    handles = []

    async def build(user_id="e2e-tester", auth_token=AUTH_TOKEN, ui_variant=None):
        """Start one isolated server and return its handle.

        Every handle gets a private database and home directory, so several
        can run at once without contending for the libSQL file lock or the
        single-instance PID guard.
        """
        # Each handle gets its own database file and private home: libSQL
        # locks the file, and the single-instance PID guard lives under
        # `$HOME/.axinite`.
        index = len(handles)
        handle_home = root / f"home-{index}"
        handle_home.mkdir()
        env = _server_env(
            mock_llm_server,
            ServerSettings(
                db_path=root / f"endpoint-setup-{index}.db",
                user_id=user_id,
                auth_token=auth_token,
                home=handle_home,
                tools_dir=tools_dir,
                ui_variant=ui_variant,
            ),
        )
        handle = AxiniteHandle(
            axinite_binary, env, root / f"server-{len(handles)}.log"
        )
        handles.append(handle)
        await handle.start()
        return handle

    try:
        yield build
    finally:
        for handle in handles:
            await handle.stop()


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
        # SANDBOX_ENABLED/ROUTINES_ENABLED above leave the jobs and routines
        # runtimes unwired, so the subsystem-availability layer would hide
        # their routes. Force the route flags on (precedence layer 1) so the
        # scenarios can still walk every nav entry.
        "FEATURE_FLAG_ROUTE_JOBS": "true",
        "FEATURE_FLAG_ROUTE_ROUTINES": "true",
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
        await pg.wait_for_selector(SEL["auth_screen"], state="hidden", timeout=15000)
        await pg.get_by_role("navigation").wait_for(state="visible", timeout=15000)
        # SolidJS exposes the stream state through its testability contract.
        # Wait for the open event before sending a message: broadcasts sent
        # before connection are not replayed to this browser.
        await pg.wait_for_selector(
            f'{SEL["sse_status"]}[data-state="connected"]',
            state="visible",
            timeout=15000,
        )
        yield pg
    finally:
        await context.close()
