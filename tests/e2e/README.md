# Axinite E2E Tests

Browser-level end-to-end tests for the Axinite web gateway using Python +
Playwright.

## Prerequisites

- Python 3.11+
- Rust toolchain (for building axinite)
- Chromium (installed via Playwright)

## Setup

```bash
cd tests/e2e
pip install -e .
playwright install chromium
```

## Build axinite

The tests need the axinite binary built with libsql support:

```bash
cargo build --no-default-features --features libsql
```

## Run tests

```bash
# From repo root
pytest tests/e2e/ -v

# Run a single scenario
pytest tests/e2e/scenarios/test_chat.py -v

# With visible browser (not headless)
HEADED=1 pytest tests/e2e/scenarios/test_connection.py -v
```

## Architecture

Tests start two subprocesses:

1. **Mock LLM** (`mock_llm.py`) -- fake OpenAI-compat server with canned
   responses
2. **Axinite** -- the real binary with gateway enabled, pointing to the mock
   LLM

Then Playwright drives a headless Chromium browser against the gateway, making
DOM assertions.

## Scenarios

| File                           | What it tests                                              |
| ------------------------------ | ---------------------------------------------------------- |
| `test_connection.py`           | Auth, route navigation, connection status                  |
| `test_chat.py`                 | Send message, SSE streaming, response rendering            |
| `test_skills.py`               | ClawHub search, skill install/remove                       |
| `test_tool_approval.py`        | Tool approval card (approve, deny, always, params toggle)  |
| `test_sse_reconnect.py`        | SSE reconnection handling                                  |
| `test_html_injection.py`       | HTML injection security                                    |
| `test_extensions_catalogue.py` | Extensions route: cards, tools, registry install, activate |
| `test_extensions_configure.py` | Extensions route: configure panel, remove                  |
| `test_extensions_channels.py`  | Extensions route: WASM channel stepper, pairing            |

## Adding new scenarios

1. Create `tests/e2e/scenarios/test_<name>.py`
2. Use the `page` fixture for a fresh browser page
3. Use selectors from `helpers.py` (update `SEL` dict if new elements are
   needed)
4. Keep tests deterministic -- use the mock LLM, not real providers

## Mocking API responses with `page.route()`

For routes that depend on external data (extensions, jobs, memory, routines),
use Playwright's `page.route()` to intercept the browser's HTTP requests to the
axinite gateway and return deterministic fixture JSON. This avoids needing real
installed binaries, live external services, or complex database setup.

### Basic pattern

```python
import json

from extensions_support import tools_tbody
from helpers import goto_route

async def test_something(page):
    # 1. Set up route intercepts BEFORE navigation triggers the fetch
    # Always use async def handlers — route.fulfill() is a coroutine and must be awaited.
    async def handle_tools(route):
        await route.fulfill(
            status=200,
            content_type="application/json",
            body=json.dumps({"tools": [{"name": "echo", "description": "Echo"}]}),
        )

    await page.route("**/api/extensions/tools", handle_tools)

    # 2. Navigate / interact to trigger the fetch
    await goto_route(page, "Extensions", "extensions")

    # 3. Assert on the rendered DOM
    rows = tools_tbody(page).locator("tr")
    assert await rows.count() == 1
```

### Matching only the exact path

`**/api/extensions` matches `http://host/api/extensions` but NOT sub-paths like
`http://host/api/extensions/install`. For the bare list endpoint, add a check
inside the handler:

```python
async def handle_ext_list(route):
    path = route.request.url.split("?")[0]
    if path.endswith("/api/extensions"):
        await route.fulfill(json={"extensions": []})
    else:
        await route.continue_()   # Let sub-paths through to the real server

await page.route("**/api/extensions*", handle_ext_list)
```

### Mocking method-specific behaviour (GET vs POST)

```python
async def handle_setup(route):
    if route.request.method == "GET":
        await route.fulfill(json={"secrets": [...]})
    else:  # POST
        await route.fulfill(json={"success": True})

await page.route("**/api/extensions/my-ext/setup", handle_setup)
```

### Counting calls (for reload tests)

```python
calls = []

async def counting_handler(route):
    calls.append(1)
    await route.fulfill(json={"extensions": []})

await page.route("**/api/extensions", counting_handler)
# ... interact ...
assert len(calls) == 2   # called twice (initial + after some action)
```

### Applying the pattern to other routes

| Route        | Key API endpoints to mock                                    |
| ------------ | ------------------------------------------------------------ |
| **Jobs**     | `/api/jobs`, `/api/jobs/{id}`, `/api/jobs/{id}/events`       |
| **Memory**   | `/api/memory/search`, `/api/memory/tree`, `/api/memory/read` |
| **Routines** | `/api/routines`, `/api/routines/{id}/runs`                   |

### Injecting state directly via `page.evaluate()`

For UI driven by server-sent events or history queries (such as the tool
approval card), intercept the history endpoint so it carries the desired state,
then trigger a refetch through the `window.__axinite.emitChatEvent` test hook:

```python
from helpers import SEL

async def handle_history(route):
    await route.fulfill(
        status=200,
        content_type="application/json",
        body=json.dumps({
            "thread_id": "e2e-approval-thread",
            "turns": [],
            "has_more": False,
            "pending_approval": {
                "request_id": "test-req-001",
                "tool_name": "shell",
                "description": "Execute: echo hello world",
                "parameters": '{"command": "echo hello world"}',
            },
        }),
    )

await page.route("**/api/chat/history*", handle_history)

# The chat surface refetches history when it receives approval_needed.
await page.evaluate("""() => window.__axinite.emitChatEvent({
    type: 'approval_needed',
    request_id: 'test-req-001',
    tool_name: 'shell',
    description: 'Execute: echo hello world',
    parameters: '{"command": "echo hello world"}'
})""")

card = page.locator(SEL["approval_card"])
await card.wait_for(state="visible")
```

This is the pattern used in `test_tool_approval.py`. The `test_extensions_*.py`
modules instead use `page.route` interception via `extensions_support.py`, not
`page.evaluate`.
