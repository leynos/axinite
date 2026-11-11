# User's guide

This guide captures operator-visible behaviour for the current Axinite runtime.

## Installation and distribution

A complete source checkout is needed to build Axinite with libSQL enabled,
because the build uses a workspace-local TLS dependency patch. Source-built
binaries distributed through GitHub Releases are supported. Cargo registry
publication and installation are unsupported because registry archives do not
carry the workspace's patch overrides. See the
[transport patch notes](../crates/hyper-rustls/UPSTREAM.md) for the dependency
rationale.

To build from source, use a complete repository checkout and install the pinned
build tools:

```sh
make install-build-tools
export PATH="$HOME/.local/bin:$PATH"
cargo build
```

This selects the dated nightly in `rust-toolchain.toml` and installs the pinned
`mold` linker on Linux. Install `clang` with your system package manager on
Linux. The installer does not change the current shell's `PATH`; the export
above makes the installed linker available to bare Cargo commands. Make targets
add the tool directory automatically. See the developers' guide for the build
standard and its deliberate exclusions.

## Skill bundle installs

Axinite now validates passive multi-file skill bundles when a skill is
installed from an HTTPS URL or catalogue download that resolves to a `.skill`
ZIP archive, or when a local `.skill` file is uploaded through the browser
Skills tab.

A valid bundle must contain exactly one shared top-level path prefix with
`SKILL.md` at `<root>/SKILL.md`. The installer also accepts optional
`references/` and `assets/` subdirectories under that same root.

The installer rejects bundles that contain unsupported top-level content, nested
`SKILL.md` files, `scripts/` or `bin/` directories, traversal paths, absolute
paths, special-file entries, duplicate normalized paths, oversized entries,
oversized archives, excessive file counts, invalid UTF-8 in `SKILL.md` or
`references/`, or executable payloads such as `.sh`, `.py`, `.js`, `.ps1`,
`.bat`, `.cmd`, `.rb`, or `.pl`.

Accepted bundles are extracted through a staged install path and only committed
into the installed-skills directory once validation and on-disk staging both
succeed. A failed bundle install should therefore leave no partial installed
skill tree behind. Bundle authors can rely on every documented entry under
`SKILL.md`, `references/`, and `assets/` being preserved byte-for-byte after a
successful install.

The browser Skills tab accepts exactly one install source at a time: a
catalogue name or slug, an HTTPS `SKILL.md` or `.skill` URL, raw `SKILL.md`
content from an API client, or one uploaded `.skill` file. Ambiguous requests,
such as sending both a name and a URL, fail before any download or install
attempt. Malformed archives report explicit `invalid_skill_bundle: ...` errors
that describe the archive-shape problem.

When a bundled skill is active, the runtime injects the selected `SKILL.md`
body into the prompt and advertises stable bundle metadata. The model-facing
active-skill block includes this required metadata contract:

- `skill`: required string. The stable skill identifier from the loaded skill
  model.
- `root`: required bundle-relative path string. The current value is `.` and it
  represents the logical root of the active skill bundle, not a host-local
  filesystem path.
- `entry`: required bundle-relative path string. The current entrypoint is
  `SKILL.md`.
- `package`: required string. The allowed values are `single_file` and
  `bundle`.

Host-local filesystem paths are not exposed as model instructions. A dedicated
`skill_read_file` tool lets the model read allowed bundle-relative files
without access to the generic filesystem tools.

The `skill_read_file` input schema requires:

```json
{
  "skill": "deploy-docs",
  "path": "references/usage.md"
}
```

The `skill` value must match the active skill identifier. The `path` value must
be one of `SKILL.md`, a file below `references/`, or a file below `assets/`.
Absolute paths, `..`, backslashes, nested `SKILL.md` files, and other roots are
rejected.

A successful text read returns inline UTF-8 content:

```json
{
  "skill": "deploy-docs",
  "path": "references/usage.md",
  "mime_type": "text/markdown",
  "content": "# Usage\n..."
}
```

Binary assets and oversized files are not returned inline in this phase. They
return a typed error with size, media type, and a stable fetch hint. Unknown
skills, missing files, disallowed paths, invalid UTF-8, and I/O failures also
return skill-scoped JSON error payloads rather than host-local paths.

## Hosted workers and remote tools

Hosted workers now advertise two tool families to the model:

- container-local development tools such as `shell`, `read_file`,
  `write_file`, `list_dir`, and `apply_patch`
- active hosted-visible orchestrator-owned tools fetched from the
  worker-authenticated remote catalogue at startup, including both Model
  Context Protocol (MCP) tools and WebAssembly (WASM) tools

When the worker receives the remote catalogue, it registers a local proxy for
each advertised remote tool. The model therefore sees the orchestrator-owned
tool's `name`, `description`, and JSON Schema parameters unchanged, even though
execution still happens in the orchestrator process.

If one of those hosted-visible WASM tools later fails, any retry guidance is
supplemental recovery help only. The worker and orchestrator still treat the
advertised `ToolDefinition.parameters` schema as the primary contract for the
first call. The same proactive first-call contract now applies in both the
local in-process path and the hosted worker path: the first tool-capable model
request already carries the advertised WASM schema before any execution attempt.

The worker now computes one registry-backed tool surface for reasoning. That
merged view is used both when the initial reasoning context is built and when
later hosted-loop iterations refresh `available_tools`, so long-running jobs do
not drift back to a local-only view. Supplemental hosted guidance from the
catalogue is injected once as a dedicated system message during context build;
later refreshes update only the tool list and any queued follow-up prompts.

### Minimal workflow example

```text
1. Worker starts and fetches the hosted-visible remote catalogue.
2. Worker registers one local proxy per advertised remote tool.
3. Model selects a hosted-visible remote tool such as `github_search`.
4. Worker sends one generic remote-tool execution request:
   POST /worker/{job_id}/tools/execute
   {
     "tool_name": "github_search",
     "params": { "query": "hosted worker transport" }
   }
5. Orchestrator validates the params against the advertised schema, executes
   the tool, and returns the normal `ToolOutput` payload.
```

The hosted-visible remote catalogue is now filtered from the canonical
`ToolRegistry` rather than from the HTTP adapter layer. That catalogue now
includes active hosted-visible MCP tools plus active hosted-visible
orchestrator-owned WASM tools that are executable in hosted mode.
Extension-management built-ins and other ineligible orchestrator-owned tools
remain outside the hosted-visible catalogue.

Not every tool in the orchestrator registry is visible in hosted mode. Tools
may still be hidden when they:

### Visibility rules & defaults

| Case                                                     | Default visibility | Expected behaviour when selected                                                                       |
| -------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------ |
| Requires interactive approval                            | Hidden             | Omitted from the remote catalogue because hosted workers cannot satisfy interactive approval prompts   |
| Depends on worker-local execution                        | Hidden             | Omitted because the orchestrator cannot safely proxy a worker-local dependency                         |
| Other ineligible, protected, or non-hosted-visible cases | Hidden             | Omitted from the remote catalogue and rejected if called directly                                      |
| Active hosted-visible MCP tool                           | Visible            | Advertised unchanged in the remote catalogue and executed through the generic remote-tool request      |
| Active hosted-visible orchestrator-owned WASM tool       | Visible            | Advertised unchanged in the remote catalogue and executed through the same generic remote-tool request |

If a hosted-visible remote tool is selected, the worker sends one generic
execution request to the orchestrator rather than using tool-family-specific
proxy routes.

## Configuration reload

On Unix, Axinite reloads configuration without a restart when it receives
`SIGHUP`; on every platform the same reload sequence is available to code that
drives the hot-reload manager directly. A reload:

1. applies secrets supplied by the configured secret injector;
2. loads the new configuration from the settings source the deployment uses. A
   database-backed deployment re-reads database settings and the default
   `config.toml` overlay; a snapshot-backed deployment re-reads `settings.json`
   and the same overlay. Both anchor on the base directory selected by the
   startup snapshot. Reload applies only the HTTP webhook subset of that
   configuration; every other resolved field, including database-backed
   settings, is discarded until the process restarts;
3. reconciles the HTTP listener when a listener controller is configured:
   restarts it when its bind address changed, restarts it when it stopped while
   the configured address stayed the same, starts it when `channels.http` is
   added, and stops it when `channels.http` is removed. Without a configured
   controller no listener work is attempted, so a reload cannot start a
   listener that startup did not create; and
4. fans out rotated or cleared webhook secrets to live channel instances without
   restarting them.

Startup config-load and reload outcomes are logged through `tracing`. Success
is logged at `debug`, and failures at `info`, so a failed reload is visible at
the default log level even when the `metrics` feature is off.

A reload never rereads process environment variables or `.env` files. Every
reload starts from the environment snapshot captured once at startup, so
environment-backed settings such as `HTTP_HOST`, `HTTP_PORT`, API keys, and
other `*_ENV`-style overrides keep their startup values until the process
restarts. To apply a change to any of those settings, edit the environment of
the service unit and restart the daemon rather than signalling it.

Reload applies only the HTTP webhook settings: it restarts or stops the
listener when the bind address or `channels.http` presence changes, and pushes
a rotated `HTTP_WEBHOOK_SECRET` to live channels. Reload can only drive the
listener controller that startup created, so enabling an HTTP listener that
startup did not start takes a restart. A non-empty `HTTP_WEBHOOK_SECRET` in the
startup environment outranks the injected secret, so remove that override and
restart before relying on store-backed rotation. Other resolved fields,
including database-backed settings, are discarded until the process restarts.
If an error aborts a reload, Axinite keeps serving the previous configuration;
check the daemon log for the reported error before restarting. A database
settings read failure does not abort the cycle:
`Config::from_db_from_context_with_toml` substitutes `Settings::default()`, so
the reload can complete with default HTTP settings.

Library embedders that assemble these runtime components themselves must pass
the same snapshot rather than letting each component capture the process
environment. `GatewayChannel::new_from_context(config, env_context)` resolves
the gateway's feature-flag overlay from the snapshot, and
`RestartTool::from_context(env)` supplies the `AXINITE_IN_DOCKER` and
`AXINITE_DISABLE_RESTART` values the restart tool uses.
`ExtensionRegistry::new_with_catalog(catalog_entries, relay_url)` and
`builtin_entries_with_relay(relay_url)` take the snapshot's relay URL the same
way. The
[0.19.0 migration notes](migrations/0.19.0.md#configuration-environment-snapshots)
list the full set of context-aware entry points.

The test helper `testing::test_utils::EnvVarsGuard` is removed. Tests that
previously used it to set process environment variables should build an
`EnvContext` with `EnvContext::default().with_env(key, value)` and pass that
snapshot to the API under test. The `test_env_context()`,
`test_env_context_with(&[(key, value)])`, and `test_config_from_context(&ctx)`
helpers cover the common cases without touching the process environment.

`bootstrap::load_axinite_env` no longer writes an auto-detected
`DATABASE_BACKEND=libsql` into the process environment. It still loads the
Axinite `.env` file, but the backend choice is applied to the snapshot by
`bootstrap::capture_config_env_context`, which sets `DATABASE_BACKEND=libsql`
when the variable is absent and the default libSQL database file exists. Code
that previously called `load_axinite_env` and then read
`std::env::var("DATABASE_BACKEND")` should call `capture_config_env_context()`
and read the value from the returned `EnvContext`, or pass that snapshot to the
consumer that needs the backend.

Library configuration loaders no longer read `.env` files. `Config::from_env()`,
`Config::from_env_with_toml()`, `Config::from_db()`, and
`Config::from_db_with_toml()` used to call `dotenvy::dotenv()` and
`bootstrap::load_axinite_env()` themselves; they now capture only the process
environment through `bootstrap::capture_config_env_context()`. The binary still
loads `./.env` and `~/.axinite/.env` at its synchronous startup boundary,
before the Tokio runtime starts, so normal deployments are unaffected. An
embedder that relied on the loaders to read those files should merge them into
a snapshot first:

1. Start from an `EnvContext` that already holds the real process environment,
   so a real environment variable still outranks either file.
2. Load `./.env` into that snapshot without overwriting existing keys, then
   apply `bootstrap::merge_axinite_env_into_context(&ctx)`, which adds
   `~/.axinite/.env` values only for keys the snapshot does not already hold.
   This preserves the startup order: process environment first, project `.env`
   next, Axinite `.env` last.
3. Pass the result to `Config::from_env_from_context(...)` or
   `Config::from_db_from_context(...)`.

Within config resolution the snapshot still outranks the TOML file, which
outranks database settings and defaults. Loading `.env` files without mutating
the process environment is tracked in
[issue #468](https://github.com/leynos/axinite/issues/468).

### Reliability metrics

Building with the optional `metrics` feature emits counters for startup
configuration loading, hot reload, secret injection, and WASM channel
credentials. Axinite does not bundle a recorder or exporter: enable the feature
only when the embedding deployment installs one, otherwise the counters are
dropped silently.

| Counter                            | `outcome` labels                                                   |
| ---------------------------------- | ------------------------------------------------------------------ |
| `axinite.startup.config_load`      | `success`, `settings_error`, `config_error`                        |
| `axinite.reload.outcomes`          | `success`, `http_removed`, `config_error`, `listener_error`        |
| `axinite.reload.secret_injection`  | `injected`, `missing`, `retained_after_error`, `error`             |
| `axinite.wasm.channel_credentials` | `injected`, `partial`, `missing`, `list_error`, `decryption_error` |

`partial` means a channel declared several credentials and only some of them
resolved; `missing` means none of the declared credentials resolved.

Every outcome is also emitted through `tracing` with an `outcome` field:
successful outcomes at `debug`, and every other outcome at `info`, so failures
stay visible at the default log level without the `metrics` feature and without
a recorder. A partial-resolution warning additionally names the channel and its
declared and resolved credential counts. Other log lines name a credential's
declared name, its channel or target environment variable, and the placeholder
it was injected as; no log line carries a secret value or decrypted material.

## Settings file

Axinite reads persisted preferences from `settings.json` in its base directory.
The base directory is `~/.axinite` unless `AXINITE_BASE_DIR` overrides it in
the startup environment. The file is optional, so a first run needs no manual
setup:

- **Missing file** — Axinite applies the built-in defaults and continues
  starting.
- **Unreadable file** — for example when the file exists but the daemon's user
  lacks read permission. Startup fails and reports
  `failed to read settings file <path>: <reason>`.
- **Malformed file** — when the file is not valid JSON, or when it is valid
  JSON whose known fields have the wrong type. Startup fails and reports
  `failed to parse settings file <path>: <reason>`.

A reported settings error is fatal rather than silently replaced by defaults,
so a corrupted file cannot quietly discard a configured database or provider.
To recover, restore the file from backup, correct the reported syntax or
permission problem, or remove the file to return to defaults and run
`axinite onboard` again. The same rules apply to a reload in a snapshot-backed
deployment: a settings file that becomes unreadable or malformed fails that
reload cycle, Axinite keeps serving the previous configuration, and the daemon
log names the offending path. A database-backed deployment reloads from the
settings table and `config.toml` and does not inspect `settings.json`.

The `settings.json` schema is tolerant of unknown keys and of keys omitted by
older releases; missing fields take their defaults. A known field whose value
has the wrong type is a schema error, not a tolerated unknown key: it is
reported like a syntax error and fails configuration loading, so correct the
invalid value rather than removing the file.

### Settings loading for library consumers

Code that embeds Axinite, including its tests, can load settings through the
same API the daemon uses. `Settings::load()` reads `settings.json` from the
ambient base directory, while `Settings::load_from(path)` reads a specific
file. Snapshot-based callers should pass
`ctx.axinite_base_dir().join("settings.json")` to `Settings::load_from` so they
read the same file as the rest of their configuration. Both return
`Result<Settings, SettingsError>`:

- A missing file yields `Settings::default()`, matching first-run behaviour.
- A file that exists but cannot be read yields `SettingsError::Read`.
- A file that exists but is not valid JSON yields `SettingsError::Parse`.

Configuration loading must propagate `SettingsError::Read` and
`SettingsError::Parse` instead of substituting defaults. Only a genuinely
absent file yields defaults; treating an unreadable or malformed file as absent
would silently discard the operator's configuration.

Diagnostic commands are the deliberate exception. `axinite doctor` and
`axinite status` must still report on a deployment whose settings file is
broken, so they print a warning and continue with `Settings::default()` rather
than aborting. That fallback is confined to diagnostics: it is not the
behaviour of configuration loading, and it must not be presented as equivalent
to a missing file.

## HTML-to-Markdown conversion for HTTP responses

When the built-in `http` tool fetches an HTML response, Axinite converts it to
Markdown before returning it to the model. This behaviour is controlled by the
`html-to-markdown` cargo feature, which is enabled by default.

The conversion uses readability-based article extraction to strip navigation,
adverts, and other boilerplate, keeping only the main article content. Embedded
media placeholders (images, audio, video, and similar embeds) are removed
rather than rendered as broken links. Article titles that the extraction step
demotes out of the body are restored as headings, and figure captions are
preserved, including a caption in the document's final section that has no
following heading to anchor it.

With `html-to-markdown-rs` 3.x, converted captions and similar text may appear
with emphasis in the Markdown, and the converter may collapse repeated
whitespace, including double spaces.

Responses whose body exceeds the 5 MB size limit are rejected regardless of
whether HTML conversion applies. When the `html-to-markdown` feature is
disabled, the raw HTML body passes through untouched.

## Self-repair notifications

Axinite can emit proactive self-repair notices when a background job remains in
the explicit `stuck` state long enough to cross the configured repair threshold.

The repair loop currently checks only jobs that the runtime has already marked
`stuck`. Once a job has remained in that state for at least
`AGENT_STUCK_THRESHOLD_SECS`, the background repair task may attempt recovery
and broadcast a notification such as:

- `Self-Repair: Job <id> was stuck for <n>s, recovery succeeded: ...`
- `Self-Repair: Job <id> was stuck for <n>s, recovery failed permanently: ...`
- `Self-Repair: Job <id> needs manual intervention: ...`
- `Self-Repair: Tool '<name>' needs manual intervention: ...`

These notices are advisory. A job success message means the runtime moved the
job back into its normal retry path. In the default production configuration,
broken tools require manual intervention because automatic tool rebuilding is
not wired to a builder. Operators should inspect the job or tool state before
retrying work.

## Workspace memory search

When workspace memory is enabled, the search backend differs by database:

- **PostgreSQL** — performs pgvector cosine-distance queries directly.
- **libSQL / Turso** — attempts an indexed `vector_top_k(...)` query when a
  compatible fixed-dimension vector index exists. After the V9 migration (which
  removed the fixed-dimension index in favour of flexible-dimension vector
  storage, with `memory_chunks.embedding` stored as a flexible vector), the
  backend automatically falls back to brute-force cosine similarity computed in
  Rust. Results from both paths feed into the same Reciprocal Rank Fusion (RRF)
  pipeline, so hybrid full-text search (FTS) + vector retrieval is preserved.

To determine which search mode is active for a workspace, run:

```text
axinite doctor
axinite status
```

Both commands report whether indexed or brute-force vector retrieval is
currently in use. See `docs/database-integrations.md` for backend trade-offs
and performance considerations.

## Browser UI

The gateway serves the SolidJS UI by default. To fall back to the previous
shell as a rollback path, set `AXINITE_WEB_UI=legacy` before starting the
gateway; the setting is read at startup.

### Signing in

On first visit the UI asks for the gateway access token (the value of
`GATEWAY_AUTH_TOKEN`) and keeps it for the browser session only. A link of the
form `/chat?token=<token>` signs in without the prompt; the UI stores the token
and removes the `token` parameter from the address bar. A token that the
gateway rejects is discarded and the prompt reappears.

### Routes and controls

The UI has chat, memory, jobs, routines, extensions, skills, and logs routes.
Each can be hidden with the flags `route_chat`, `route_memory`, `route_jobs`,
`route_routines`, `route_extensions`, `route_skills`, and `route_logs`; the log
panel is hidden separately by `panel_logs`.

- Restart control: a button in the header, with a confirmation dialog, that
  sends `/restart` to the agent and waits for the gateway to go down and come
  back. It is not feature-flagged; it appears only when the gateway reports
  restart support, which requires `AXINITE_IN_DOCKER=true` in its environment.
  If the command fails, the UI shows a retryable alert and changes the button
  to Retry restart. The browser console records only `restart_failed` and its
  bounded category (`timeout`, `transport`, or `http`); it does not log
  response bodies or URLs.
- TEE attestation: a header indicator whose popover fetches the full
  attestation report and copies it to the clipboard. It requires the
  `surface_tee_attestation` flag and a non-local hostname (the report is served
  from `api.<domain>`), so it stays hidden on `localhost` and IP addresses.
- Extension pairing and activation: on the extensions route, a three-step
  stepper shows the activation progress of WebAssembly (WASM) channels, and
  pending pairing requests for a channel can be approved. Neither has its own
  flag; both follow `route_extensions`.

### Feature flags

The UI reads its flags from `GET /api/features`. Each flag resolves in this
order, highest first:

1. The `FEATURE_FLAG_<NAME>` environment variable, read when the process first
   resolves flags. `true` (in any case) enables the flag; any other set value
   disables it. Set it before starting the gateway.
2. A deployment override, set without a restart through
   `PUT /api/settings/feature_flag:<name>`.
3. Subsystem availability: a flag whose backing subsystem is not running (for
   example `route_jobs` without a jobs runtime) is switched off. This layer can
   only disable a flag, never enable one.
4. The compiled default.

Reads of `GET /api/features` take an optional `X-Deployment-Id` header that
defaults to `default`. Writes require it. The value must be 1 to 64 lowercase
letters, digits, or underscores; anything else is rejected with 400.

The gateway requires `Authorization: Bearer <GATEWAY_AUTH_TOKEN>` for API
requests. The SPA currently reads the `default` deployment; API clients can
select another deployment by sending the same `X-Deployment-Id` header on both
reads and writes. For example, this disables the logs route for the SPA's
default deployment:

```sh
curl -X PUT http://127.0.0.1:3000/api/settings/feature_flag:route_logs \
  -H "Authorization: Bearer $GATEWAY_AUTH_TOKEN" \
  -H "X-Deployment-Id: default" \
  -H "Content-Type: application/json" \
  -d '{"value": false}'
```

The value must be a JSON boolean or the string `"true"` or `"false"`
(case-insensitive). Flag names use lowercase letters, digits, and underscores.
The gateway stores overrides per deployment and applies writes without a
restart. `FEATURE_FLAG_<NAME>` environment variables in the startup environment
snapshot take precedence over stored values; as with other environment-backed
settings, changing them requires a process restart rather than a reload. To
remove a persisted override, delete its row from `feature_flag_overrides`, then
restart each gateway process that has cached that deployment. Deleting the row
alone does not clear its in-memory override. The settings `DELETE` route rejects
`feature_flag:` keys with `400 Bad Request`.

If the gateway cannot load persisted overrides, `GET /api/features` returns
`503 Service Unavailable`. The SPA continues with browser-local overrides and
compiled defaults, and records a generic failure in the browser console. Check
gateway logs and database availability before relying on those displayed flags.

### Trying the UI without the daemon

`make frontend-stub` serves the UI against a stubbed backend, with no daemon
needed. See `docs/solidjs-frontend.md` for developer details, including the
stub's limitations.
