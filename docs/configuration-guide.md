# Axinite configuration guide

This guide is for operators and contributors who need a complete reference
for the current `ironclaw` command-line interface (CLI) surface and the
environment variables that shape axinite at runtime. The system narrative uses
the name axinite, but commands, APIs, and filenames retain `ironclaw` where
that is still the implemented identifier.

## 1. Configuration sources and precedence

Axinite does not read configuration from a single source. Startup is layered so
the process can bootstrap before the database is available, then reload richer
settings once persistent state is online.

Table 1. Effective configuration precedence.

| Priority | Source | Notes |
|----------|--------|-------|
| Highest | Explicit process environment | Values exported in the shell always win. Empty strings are treated as unset by the config helpers. |
| High | `./.env` | Loaded by `dotenvy::dotenv()` before `~/.ironclaw/.env`, and never overwrites existing process variables. |
| Medium | `~/.ironclaw/.env` | Loaded by `crate::bootstrap::load_ironclaw_env()`. This is the bootstrap layer for settings needed before database access, especially `DATABASE_URL`. |
| Medium | Optional TOML overlay | `Config::from_env_with_toml()` and `Config::from_db_with_toml()` overlay `~/.ironclaw/config.toml` by default, or an explicit `--config` path when one is provided. TOML overrides JSON or database settings, but still loses to environment variables. |
| Low | Persisted database settings | Used once the settings store is available. |
| Lowest | Code defaults | Hard-coded defaults in `src/config/*.rs`. |

Two bootstrap details matter in practice:

1. `IRONCLAW_BASE_DIR` changes the per-user base directory from
   `~/.ironclaw` to another path. Relative paths are accepted, but the code
   warns when one is used.
1. If `DATABASE_BACKEND` is still unset after environment files are loaded and
   `~/.ironclaw/ironclaw.db` exists, startup auto-selects the `libsql`
   backend.

## 2. Global `ironclaw` CLI options

These options apply to the root parser and are available before any
subcommand.

Table 2. Global CLI options.

| Option | Meaning | Default or behaviour |
|--------|---------|----------------------|
| `--cli-only` | Run in interactive CLI mode only and disable other channels. | Disabled by default. |
| `--no-db` | Skip database connection. | Disabled by default. Intended mainly for testing and reduced bootstrap paths. |
| `-m`, `--message <TEXT>` | Send one message and exit. | Omitted by default. |
| `-c`, `--config <PATH>` | Load an explicit TOML configuration file. | If omitted, startup looks for the default TOML path under `~/.ironclaw`. |
| `--no-onboard` | Skip the first-run onboarding check. | Disabled by default. |

When no subcommand is supplied, `ironclaw` behaves as `ironclaw run`.

## 3. Command reference

### 3.1 Top-level commands

Table 3. Top-level commands exposed by `ironclaw`.

| Command | Purpose | Notes |
|---------|---------|-------|
| `run` | Start the main agent runtime. | This is the default path when no subcommand is given. |
| `onboard` | Run the interactive setup wizard. | Supports a small set of mutually exclusive shortcuts. |
| `config` | Inspect or mutate persisted configuration values. | Subcommands listed below. |
| `tool` | Manage installed WASM tools. | Subcommands listed below. |
| `registry` | Browse and install registry entries. | Covers tools, channels, and skills exposed through the registry. |
| `mcp` | Manage Model Context Protocol (MCP) servers. | Supports HTTP, stdio, and Unix socket transports. |
| `memory` | Search and edit workspace memory. | Operates against the configured database-backed workspace store. |
| `pairing` | Approve inbound direct-message pairing requests. | Used for channels that gate unknown senders. |
| `service` | Install and manage the operating-system service wrapper. | Targets `launchd` or `systemd`, depending on platform. |
| `doctor` | Probe external dependencies and validate configuration. | No extra command-specific flags. |
| `status` | Show system status and diagnostics. | No extra command-specific flags. |
| `completion` | Generate shell completion scripts. | Requires `--shell`. |
| `import` | Import data from other systems. | Present only when the `import` feature is enabled. |
| `worker` | Run the internal container worker entrypoint. | Hidden from normal help output. |
| `claude-bridge` | Run the internal Claude Code bridge container entrypoint. | Hidden from normal help output. |

### 3.2 `ironclaw onboard`

Table 4. `ironclaw onboard` options.

| Option | Meaning | Notes |
|--------|---------|-------|
| `--skip-auth` | Reuse existing authentication instead of re-running auth. | Can be combined with normal onboarding. |
| `--channels-only` | Reconfigure channels only. | Conflicts with `--provider-only` and `--quick`. |
| `--provider-only` | Reconfigure only the LLM provider and model. | Conflicts with `--channels-only` and `--quick`. |
| `--quick` | Accept defaults for everything except the LLM provider and model. | Conflicts with `--channels-only` and `--provider-only`. |

### 3.3 `ironclaw config`

Table 5. `ironclaw config` subcommands.

| Command | Syntax | Meaning |
|---------|--------|---------|
| `init` | `ironclaw config init [-o|--output PATH] [--force]` | Emit a starter configuration file. |
| `list` | `ironclaw config list [-f|--filter PREFIX]` | List known configuration paths, optionally filtered by prefix. |
| `get` | `ironclaw config get <path>` | Print one configuration value. |
| `set` | `ironclaw config set <path> <value>` | Persist one configuration value. |
| `reset` | `ironclaw config reset <path>` | Remove a persisted override for one configuration path. |
| `path` | `ironclaw config path` | Print the default TOML path. |

Table 6. `ironclaw config` options.

| Option | Used by | Meaning |
|--------|---------|---------|
| `-o`, `--output <PATH>` | `config init` | Write the starter file to an explicit path. |
| `--force` | `config init` | Overwrite the output file if it already exists. |
| `-f`, `--filter <PREFIX>` | `config list` | Restrict listed keys to a prefix. |

### 3.4 `ironclaw tool`

Table 7. `ironclaw tool` subcommands and options.

| Command | Syntax | Meaning |
|---------|--------|---------|
| `install` | `ironclaw tool install <path> [-n|--name NAME] [--capabilities PATH] [-t|--target PATH] [--release <BOOL>] [--skip-build] [-f|--force]` | Install a WASM tool from a file or a crate directory. |
| `list` | `ironclaw tool list [-d|--dir PATH] [-v|--verbose]` | List installed tools. |
| `remove` | `ironclaw tool remove <name> [-d|--dir PATH]` | Remove an installed tool. |
| `info` | `ironclaw tool info <name-or-path> [-d|--dir PATH]` | Show metadata for a tool. |
| `auth` | `ironclaw tool auth <name> [-d|--dir PATH] [-u|--user USER]` | Trigger tool authentication for one user. |
| `setup` | `ironclaw tool setup <name> [-d|--dir PATH] [-u|--user USER]` | Run tool setup without a full install. |

Table 8. `ironclaw tool` option meanings.

| Option | Meaning | Notes |
|--------|---------|-------|
| `-n`, `--name <NAME>` | Override the install name. | `tool install` only. |
| `--capabilities <PATH>` | Use an explicit capabilities file. | `tool install` only. |
| `-t`, `--target <PATH>` | Override the install target directory. | `tool install` only. |
| `--release <BOOL>` | Build in release mode before install. | Defaults to `true`. |
| `--skip-build` | Skip the build step and install an existing artefact. | `tool install` only. |
| `-f`, `--force` | Replace an existing installation. | `tool install` only. |
| `-d`, `--dir <PATH>` | Operate on an explicit tool directory instead of the configured default. | Used by `list`, `remove`, `info`, `auth`, and `setup`. |
| `-v`, `--verbose` | Show extra metadata. | `tool list` only. |
| `-u`, `--user <USER>` | Select the user identity for auth or setup. | Defaults to `default`. |

### 3.5 `ironclaw registry`

Table 9. `ironclaw registry` subcommands and options.

| Command | Syntax | Meaning |
|---------|--------|---------|
| `list` | `ironclaw registry list [-k|--kind KIND] [-t|--tag TAG] [-v|--verbose]` | List registry entries. |
| `info` | `ironclaw registry info <name>` | Show one registry entry. |
| `install` | `ironclaw registry install <name> [-f|--force] [--build]` | Install one registry entry. |
| `install-defaults` | `ironclaw registry install-defaults [-f|--force] [--build]` | Install the default registry set. |

Table 10. `ironclaw registry` option meanings.

| Option | Meaning | Notes |
|--------|---------|-------|
| `-k`, `--kind <KIND>` | Filter registry results by entry kind. | `registry list` only. |
| `-t`, `--tag <TAG>` | Filter registry results by tag. | `registry list` only. |
| `-v`, `--verbose` | Show extra entry metadata. | `registry list` only. |
| `-f`, `--force` | Replace an existing installation. | `registry install` and `registry install-defaults`. |
| `--build` | Build the extension from source if needed. | `registry install` and `registry install-defaults`. |

### 3.6 `ironclaw mcp`

Table 11. `ironclaw mcp` subcommands.

| Command | Syntax | Meaning |
|---------|--------|---------|
| `add` | `ironclaw mcp add <name> [url] [--transport http|stdio|unix] [--command CMD] [--arg ARG ...] [--env KEY=VALUE ...] [--socket PATH] [--header KEY:VALUE ...] [--client-id ID] [--auth-url URL] [--token-url URL] [--scopes CSV] [--description TEXT]` | Register an MCP server definition. |
| `remove` | `ironclaw mcp remove <name>` | Remove an MCP server definition. |
| `list` | `ironclaw mcp list [-v|--verbose]` | List registered MCP servers. |
| `auth` | `ironclaw mcp auth <name> [-u|--user USER]` | Authenticate one MCP server for one user. |
| `test` | `ironclaw mcp test <name> [-u|--user USER]` | Test one MCP server for one user. |
| `toggle` | `ironclaw mcp toggle <name> [--enable|--disable]` | Enable or disable one MCP server definition. |

Table 12. `ironclaw mcp` option meanings.

| Option | Meaning | Notes |
|--------|---------|-------|
| `--transport <http|stdio|unix>` | Select the transport kind. | Defaults to `http` when omitted. |
| `--command <CMD>` | Executable to launch for stdio transport. | `mcp add` only. |
| `--arg <ARG>` | One or more command arguments. | `mcp add` only, repeatable. |
| `--env <KEY=VALUE>` | Environment variable to inject into the child process. | `mcp add` only, repeatable. |
| `--socket <PATH>` | Unix socket path. | `mcp add` only. |
| `--header <KEY:VALUE>` | Additional HTTP header. | `mcp add` only, repeatable. |
| `--client-id <ID>` | OAuth client identifier. | `mcp add` only. |
| `--auth-url <URL>` | OAuth authorization endpoint. | `mcp add` only. |
| `--token-url <URL>` | OAuth token endpoint. | `mcp add` only. |
| `--scopes <CSV>` | Comma-separated OAuth scopes. | `mcp add` only. |
| `--description <TEXT>` | Human-readable description. | `mcp add` only. |
| `-v`, `--verbose` | Show extra metadata. | `mcp list` only. |
| `-u`, `--user <USER>` | Select the user identity. | `mcp auth` and `mcp test`, default `default`. |
| `--enable` | Enable the target server. | `mcp toggle` only, conflicts with `--disable`. |
| `--disable` | Disable the target server. | `mcp toggle` only, conflicts with `--enable`. |

### 3.7 `ironclaw memory`

Table 13. `ironclaw memory` subcommands and options.

| Command | Syntax | Meaning |
|---------|--------|---------|
| `search` | `ironclaw memory search <query> [-l|--limit N]` | Search workspace memory. |
| `read` | `ironclaw memory read <path>` | Read one memory document. |
| `write` | `ironclaw memory write <path> [content] [-a|--append]` | Write or append a memory document. |
| `tree` | `ironclaw memory tree [path] [-d|--depth N]` | Print a tree view of memory paths. |
| `status` | `ironclaw memory status` | Show memory subsystem status. |

Table 14. `ironclaw memory` option meanings.

| Option | Meaning | Notes |
|--------|---------|-------|
| `-l`, `--limit <N>` | Maximum number of search results. | Defaults to `5`. |
| `-a`, `--append` | Append instead of replacing the file. | `memory write` only. |
| `-d`, `--depth <N>` | Limit tree traversal depth. | Defaults to `3`. |

### 3.8 `ironclaw pairing`, `service`, `completion`, `doctor`, and `status`

Table 15. Remaining user-facing commands.

| Command | Syntax | Meaning |
|---------|--------|---------|
| `pairing list` | `ironclaw pairing list <channel> [--json]` | List pending pairing requests for one channel. |
| `pairing approve` | `ironclaw pairing approve <channel> <code>` | Approve one pairing request. |
| `service install` | `ironclaw service install` | Install the operating-system service. |
| `service start` | `ironclaw service start` | Start the service. |
| `service stop` | `ironclaw service stop` | Stop the service. |
| `service status` | `ironclaw service status` | Show service status. |
| `service uninstall` | `ironclaw service uninstall` | Remove the service. |
| `completion` | `ironclaw completion --shell <bash|elvish|fish|powershell|zsh>` | Generate shell completions. |
| `doctor` | `ironclaw doctor` | Run dependency and configuration diagnostics. |
| `status` | `ironclaw status` | Show runtime status and diagnostics. |

### 3.9 Feature-gated and hidden commands

Table 16. Feature-gated and hidden commands.

| Command | Syntax | Meaning |
|---------|--------|---------|
| `import openclaw` | `ironclaw import openclaw [--path PATH] [--dry-run] [--re-embed] [--user-id USER]` | Import from an OpenClaw data source when the `import` feature is enabled. |
| `worker` | `ironclaw worker --job-id UUID [--orchestrator-url URL] [--max-iterations N]` | Internal sandbox worker entrypoint. Defaults to `http://host.docker.internal:50051` and `50`. |
| `claude-bridge` | `ironclaw claude-bridge --job-id UUID [--orchestrator-url URL] [--max-turns N] [--model MODEL]` | Internal Claude Code bridge entrypoint. Defaults to `http://host.docker.internal:50051`, `50`, and `sonnet`. |

## 4. Environment variables

### 4.1 Bootstrap, filesystem, and configuration overlays

Table 17. Bootstrap and configuration-source environment variables.

| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `IRONCLAW_BASE_DIR` | Override the per-user base directory. | Defaults to `~/.ironclaw`. An empty string is treated as unset. |
| `DATABASE_BACKEND` | Select the database backend. | Accepted values include `postgres`, `postgresql`, `pg`, `libsql`, `turso`, and `sqlite`. Defaults to `postgres` unless libSQL auto-detection triggers. |
| `DATABASE_URL` | PostgreSQL connection string. | Required unless the effective backend is `libsql`. |
| `ONBOARD_COMPLETED` | Mark onboarding as complete for first-run checks. | Written to `~/.ironclaw/.env` by the setup wizard. The first-run check treats `true` as complete. |
| `WORKSPACE_IMPORT_DIR` | Import workspace files from a directory before built-in seeding. | Optional. Files are imported only when they do not already exist in the workspace store. |
| `OBSERVABILITY_BACKEND` | Select the observer backend. | `none` or `noop` discard events; `log` emits them via `tracing`. Unknown values currently fall back to `noop`. |

### 4.2 Database and secrets

Table 18. Database and secrets environment variables.

| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `DATABASE_POOL_SIZE` | PostgreSQL connection-pool size. | Defaults to `10`. |
| `DATABASE_SSLMODE` | PostgreSQL TLS mode. | `disable`, `prefer`, or `require`; defaults to `prefer`. |
| `LIBSQL_PATH` | Local libSQL database path. | Defaults to `~/.ironclaw/ironclaw.db` when `DATABASE_BACKEND=libsql`. |
| `LIBSQL_URL` | Remote libSQL or Turso sync URL. | Optional. |
| `LIBSQL_AUTH_TOKEN` | Auth token for `LIBSQL_URL`. | Required when `LIBSQL_URL` is set. |
| `SECRETS_MASTER_KEY` | Master key for encrypted secrets storage. | Optional, but must be at least 32 bytes when set. If omitted, axinite falls back to the operating-system keychain when available. |

### 4.3 Agent runtime, safety, routines, heartbeat, hygiene, skills, and builder mode

Table 19. Core runtime behaviour variables.

| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `AGENT_NAME` | Agent display name. | Falls back to the persisted setting. |
| `AGENT_MAX_PARALLEL_JOBS` | Maximum concurrent jobs. | Falls back to the persisted setting. |
| `AGENT_JOB_TIMEOUT_SECS` | Per-job timeout. | Falls back to the persisted setting. |
| `AGENT_STUCK_THRESHOLD_SECS` | Threshold for treating a job as stuck. | Falls back to the persisted setting. |
| `SELF_REPAIR_CHECK_INTERVAL_SECS` | Interval between self-repair checks. | Falls back to the persisted setting. |
| `SELF_REPAIR_MAX_ATTEMPTS` | Maximum self-repair attempts. | Falls back to the persisted setting. |
| `AGENT_USE_PLANNING` | Enable planning before tool execution. | Boolean; falls back to the persisted setting. |
| `SESSION_IDLE_TIMEOUT_SECS` | Session idle timeout before pruning. | Falls back to the persisted setting. |
| `ALLOW_LOCAL_TOOLS` | Allow direct local shell or filesystem tools outside the sandbox. | Boolean, default `false`. |
| `MAX_COST_PER_DAY_CENTS` | Daily spending cap for model usage. | Optional integer; unset means unlimited. |
| `MAX_ACTIONS_PER_HOUR` | Hourly cap for model and tool actions. | Optional integer; unset means unlimited. |
| `AGENT_MAX_TOOL_ITERATIONS` | Maximum tool-loop iterations per job. | Falls back to the persisted setting. |
| `AGENT_AUTO_APPROVE_TOOLS` | Skip tool approvals entirely. | Boolean; meant for benchmarks or CI-style runs. |
| `DEFAULT_TIMEZONE` | Default timezone for new sessions. | Must be a valid IANA timezone. |
| `AGENT_MAX_TOKENS_PER_JOB` | Maximum tokens per job. | `0` means unlimited. |
| `SAFETY_MAX_OUTPUT_LENGTH` | Maximum captured tool-output length. | Defaults to `100000`. |
| `SAFETY_INJECTION_CHECK_ENABLED` | Enable prompt-injection checks. | Boolean, default `true`. |
| `ROUTINES_ENABLED` | Enable the routines subsystem. | Boolean, default `true`. |
| `ROUTINES_CRON_INTERVAL` | Poll interval for cron-style routines, in seconds. | Defaults to `15`. |
| `ROUTINES_MAX_CONCURRENT` | Maximum concurrent routines across all users. | Defaults to `10`. |
| `ROUTINES_DEFAULT_COOLDOWN` | Default cooldown between routine firings, in seconds. | Defaults to `300`. |
| `ROUTINES_MAX_TOKENS` | Maximum tokens for lightweight routine model calls. | Defaults to `4096`. |
| `ROUTINES_LIGHTWEIGHT_TOOLS` | Allow tool use in lightweight routines. | Boolean, default `true`. |
| `ROUTINES_LIGHTWEIGHT_MAX_ITERATIONS` | Maximum tool iterations for lightweight routines. | Defaults to `3`, capped at `5` even if a larger value is supplied. |
| `HEARTBEAT_ENABLED` | Enable heartbeat checks. | Boolean; defaults from persisted settings, otherwise `false`. |
| `HEARTBEAT_INTERVAL_SECS` | Heartbeat interval in seconds. | Defaults from persisted settings, otherwise `1800`. |
| `HEARTBEAT_NOTIFY_CHANNEL` | Channel to notify for heartbeat findings. | Optional. |
| `HEARTBEAT_NOTIFY_USER` | User ID to notify for heartbeat findings. | Optional. |
| `HEARTBEAT_QUIET_START` | Start hour for quiet hours. | Optional integer `0` to `23`. |
| `HEARTBEAT_QUIET_END` | End hour for quiet hours. | Optional integer `0` to `23`. |
| `HEARTBEAT_TIMEZONE` | Timezone for quiet-hours evaluation. | Must be a valid IANA timezone when set. |
| `MEMORY_HYGIENE_ENABLED` | Enable automatic workspace hygiene. | Boolean, default `true`. |
| `MEMORY_HYGIENE_DAILY_RETENTION_DAYS` | Retention for `daily/` memory documents. | Defaults to `30`. |
| `MEMORY_HYGIENE_CONVERSATION_RETENTION_DAYS` | Retention for `conversations/` memory documents. | Defaults to `7`. |
| `MEMORY_HYGIENE_CADENCE_HOURS` | Minimum interval between hygiene passes. | Defaults to `12`. |
| `SKILLS_ENABLED` | Enable the skills subsystem. | Boolean, default `true`. |
| `SKILLS_DIR` | Directory for locally placed skills. | Defaults to `~/.ironclaw/skills`. |
| `SKILLS_INSTALLED_DIR` | Directory for registry-installed skills. | Defaults to `~/.ironclaw/installed_skills`. |
| `SKILLS_MAX_ACTIVE` | Maximum simultaneously active skills. | Defaults to `3`. |
| `SKILLS_MAX_CONTEXT_TOKENS` | Maximum prompt-budget tokens allocated to skills. | Defaults to `4000`. |
| `BUILDER_ENABLED` | Enable builder mode. | Boolean, default `true`. |
| `BUILDER_DIR` | Directory for builder artefacts. | Defaults to the process temporary directory when unset. |
| `BUILDER_MAX_ITERATIONS` | Maximum builder iterations. | Defaults to `20`. |
| `BUILDER_TIMEOUT_SECS` | Builder timeout in seconds. | Defaults to `600`. |
| `BUILDER_AUTO_REGISTER` | Auto-register newly built WASM tools. | Boolean, default `true`. |

### 4.4 Channels, gateway, tunnelling, and relay

Table 20. Channel, gateway, tunnel, and relay environment variables.

| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `CLI_ENABLED` | Enable the CLI channel. | Truthy by default; `false` or `0` disables it. |
| `HTTP_HOST` | Bind address for the HTTP and webhook channel. | Only enabling HTTP when `HTTP_HOST` or `HTTP_PORT` is set. Defaults to `0.0.0.0` once enabled. |
| `HTTP_PORT` | Port for the HTTP and webhook channel. | Enables HTTP when set; defaults to `8080` once enabled. |
| `HTTP_WEBHOOK_SECRET` | Shared secret for validating webhook traffic. | Optional, and by itself does not enable the HTTP listener. |
| `HTTP_USER_ID` | User ID associated with the HTTP channel. | Defaults to `http`. |
| `GATEWAY_ENABLED` | Enable the browser gateway channel. | Boolean, default `true`. |
| `GATEWAY_HOST` | Bind address for the web gateway. | Defaults to `127.0.0.1`. |
| `GATEWAY_PORT` | Port for the web gateway. | Defaults to `3000`. |
| `GATEWAY_AUTH_TOKEN` | Bearer token for the gateway. | Optional; generated at runtime if omitted. |
| `GATEWAY_USER_ID` | User ID associated with the gateway. | Defaults to `default`. |
| `SIGNAL_HTTP_URL` | Base URL for the `signal-cli` HTTP endpoint. | Optional. Enables Signal support when set. |
| `SIGNAL_ACCOUNT` | Account identifier for Signal. | Required when `SIGNAL_HTTP_URL` is set. |
| `SIGNAL_ALLOW_FROM` | Comma-separated allowlist for direct messages. | Defaults to the configured Signal account, which effectively self-allowlists only that account until changed. |
| `SIGNAL_ALLOW_FROM_GROUPS` | Comma-separated allowlist for group IDs. | Empty by default. |
| `SIGNAL_DM_POLICY` | Direct-message policy. | `open`, `allowlist`, or `pairing`; defaults to `pairing`. |
| `SIGNAL_GROUP_POLICY` | Group-message policy. | `allowlist`, `open`, or `disabled`; defaults to `allowlist`. |
| `SIGNAL_GROUP_ALLOW_FROM` | Comma-separated allowlist for senders inside groups. | Empty by default, which causes the runtime to inherit `SIGNAL_ALLOW_FROM`. |
| `SIGNAL_IGNORE_ATTACHMENTS` | Ignore messages that only contain attachments. | Accepts `true` or `1`; defaults to `false`. |
| `SIGNAL_IGNORE_STORIES` | Ignore Signal story messages. | Accepts `true` or `1`; defaults to `true`. |
| `WASM_CHANNELS_DIR` | Directory containing installed WASM channels. | Defaults to `~/.ironclaw/channels`. |
| `WASM_CHANNELS_ENABLED` | Enable WASM channels. | Boolean, default `true`. |
| `TELEGRAM_OWNER_ID` | Back-compat single-owner Telegram override. | Optional integer. Injects a telegram-only owner ID into the channel settings map. |
| `TUNNEL_URL` | Static public HTTPS URL for webhook-capable channels. | Optional, but must start with `https://` when set. |
| `TUNNEL_PROVIDER` | Managed tunnel provider name. | Optional. `none` or an empty value disables managed tunnelling. |
| `TUNNEL_CF_TOKEN` | Cloudflare tunnel token. | Used when the provider is Cloudflare. |
| `TUNNEL_TS_FUNNEL` | Enable Tailscale Funnel mode. | Accepts `true` or `1`; otherwise false. |
| `TUNNEL_TS_HOSTNAME` | Tailscale hostname. | Optional. |
| `TUNNEL_NGROK_DOMAIN` | Reserved ngrok domain. | Optional. |
| `TUNNEL_NGROK_TOKEN` | ngrok auth token. | Optional. Passed to the ngrok child process as `NGROK_AUTHTOKEN`. |
| `TUNNEL_CUSTOM_HEALTH_URL` | Health probe URL for a custom tunnel launcher. | Optional. |
| `TUNNEL_CUSTOM_URL_PATTERN` | Pattern for extracting the public URL from custom tunnel output. | Optional. |
| `TUNNEL_CUSTOM_COMMAND` | Command used to launch a custom tunnel. | Optional. |
| `CHANNEL_RELAY_URL` | Base URL for the external channel relay service. | The relay integration is enabled only when both this variable and `CHANNEL_RELAY_API_KEY` are set. |
| `CHANNEL_RELAY_API_KEY` | API key for the relay service. | Required with `CHANNEL_RELAY_URL`. |
| `IRONCLAW_OAUTH_CALLBACK_URL` | Override the OAuth callback base URL. | Optional. Also used to decide whether OAuth should be routed through the web gateway instead of a loopback listener. |
| `IRONCLAW_INSTANCE_ID` | Override the relay instance identifier. | Optional. |
| `RELAY_REQUEST_TIMEOUT_SECS` | Relay HTTP request timeout. | Defaults to `30`. |
| `RELAY_STREAM_TIMEOUT_SECS` | Relay long-poll or stream timeout. | Defaults to `86400`. |
| `RELAY_BACKOFF_INITIAL_MS` | Initial exponential-backoff interval for relay retries. | Defaults to `1000`. |
| `RELAY_BACKOFF_MAX_MS` | Maximum exponential-backoff interval for relay retries. | Defaults to `60000`. |

### 4.5 LLM selection, failover, embeddings, and transcription

Table 21. LLM routing and provider-selection environment variables.

| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `LLM_BACKEND` | Select the primary LLM backend. | Defaults to `nearai`. Known backends include `nearai`, `bedrock`, and the provider IDs in `providers.json`. Unknown values fall back to the generic OpenAI-compatible provider definition. |
| `LLM_REQUEST_TIMEOUT_SECS` | End-to-end request timeout for LLM calls. | Defaults to `120`. |
| `NEARAI_AUTH_URL` | OAuth or auth base URL for the NEAR AI session manager. | Defaults to `https://private.near.ai`. |
| `NEARAI_SESSION_PATH` | Session file path for NEAR AI auth state. | Defaults to `~/.ironclaw/session.json`. |
| `NEARAI_SESSION_TOKEN` | Inject a NEAR AI session token directly through the environment. | Optional. Takes precedence over any token loaded from `NEARAI_SESSION_PATH`. |
| `NEARAI_API_KEY` | NEAR AI API key. | Optional. When present, the default NEAR AI base URL switches to the cloud endpoint. |
| `NEARAI_MODEL` | Primary NEAR AI model. | Defaults to `zai-org/GLM-latest`. |
| `NEARAI_CHEAP_MODEL` | Lower-cost NEAR AI model for routing. | Optional. |
| `NEARAI_BASE_URL` | NEAR AI API base URL. | Defaults to `https://cloud-api.near.ai` when `NEARAI_API_KEY` is set, otherwise `https://private.near.ai`. |
| `NEARAI_FALLBACK_MODEL` | Explicit fallback model for NEAR AI. | Optional. |
| `NEARAI_MAX_RETRIES` | Retry count for NEAR AI requests. | Defaults to `3`. |
| `CIRCUIT_BREAKER_THRESHOLD` | Error threshold for the NEAR AI circuit breaker. | Optional positive integer. |
| `CIRCUIT_BREAKER_RECOVERY_SECS` | NEAR AI circuit-breaker recovery window. | Defaults to `30`. |
| `RESPONSE_CACHE_ENABLED` | Enable the NEAR AI response cache. | Boolean, default `false`. |
| `RESPONSE_CACHE_TTL_SECS` | Response-cache entry time to live. | Defaults to `3600`. |
| `RESPONSE_CACHE_MAX_ENTRIES` | Maximum number of cache entries. | Defaults to `1000`. |
| `LLM_FAILOVER_COOLDOWN_SECS` | Cooldown window before a provider is retried after repeated failures. | Defaults to `300`. |
| `LLM_FAILOVER_THRESHOLD` | Failure count before failover activates. | Defaults to `3`. |
| `SMART_ROUTING_CASCADE` | Enable smart-routing cascade behaviour. | Boolean, default `true`. |
| `BEDROCK_REGION` | AWS region for the Bedrock backend. | Defaults to `us-east-1`. |
| `BEDROCK_MODEL` | Bedrock model identifier. | Required when `LLM_BACKEND=bedrock`. |
| `BEDROCK_CROSS_REGION` | Cross-region routing scope for Bedrock. | Optional, but must be one of `us`, `eu`, `apac`, or `global`. |
| `AWS_PROFILE` | Named AWS profile for Bedrock authentication. | Optional. |
| `ANTHROPIC_OAUTH_TOKEN` | OAuth token used by Anthropic-backed providers. | Optional. |
| `ANTHROPIC_CACHE_RETENTION` | Anthropic cache-retention mode. | Optional. |
| `EMBEDDING_ENABLED` | Enable embeddings. | Boolean; defaults from persisted settings, otherwise `false`. |
| `EMBEDDING_PROVIDER` | Embeddings backend. | Supports `openai`, `nearai`, and `ollama`. |
| `EMBEDDING_MODEL` | Embedding model name. | Falls back to the persisted setting. |
| `OLLAMA_BASE_URL` | Ollama endpoint for embeddings or Ollama-backed chat providers. | Defaults to `http://localhost:11434` when needed. |
| `EMBEDDING_DIMENSION` | Explicit embedding dimension. | Optional. Defaults are inferred from the model name, with `1536` as the fallback for unknown models. |
| `TRANSCRIPTION_ENABLED` | Enable audio transcription. | Boolean; defaults from persisted settings, otherwise `false`. |
| `TRANSCRIPTION_PROVIDER` | Transcription provider. | Currently only `openai` is implemented, and that is the default. |
| `TRANSCRIPTION_MODEL` | Transcription model. | Defaults to `whisper-1`. |
| `TRANSCRIPTION_BASE_URL` | Override the transcription API base URL. | Optional. |

### 4.6 Built-in registry provider environment variables

The shipped `providers.json` file defines the built-in non-Bedrock,
non-NEAR-AI provider backends. Operators can override or extend this file with
`~/.ironclaw/providers.json`, in which case the environment-variable names come
from the custom provider definition rather than this table.

Table 22. Built-in provider-specific environment variables from `providers.json`.

| Provider ID | API key env | Base URL env | Model env | Extra headers env | Default base URL | Default model |
|-------------|-------------|--------------|-----------|-------------------|------------------|---------------|
| `openai` | `OPENAI_API_KEY` | `OPENAI_BASE_URL` | `OPENAI_MODEL` | | provider default | `gpt-5-mini` |
| `anthropic` | `ANTHROPIC_API_KEY` | `ANTHROPIC_BASE_URL` | `ANTHROPIC_MODEL` | | provider default | `claude-sonnet-4-20250514` |
| `ollama` | | `OLLAMA_BASE_URL` | `OLLAMA_MODEL` | | `http://localhost:11434` | `llama3` |
| `openai_compatible` | `LLM_API_KEY` | `LLM_BASE_URL` | `LLM_MODEL` | `LLM_EXTRA_HEADERS` | none | `default` |
| `tinfoil` | `TINFOIL_API_KEY` | | `TINFOIL_MODEL` | | `https://inference.tinfoil.sh/v1` | `kimi-k2-5` |
| `openrouter` | `OPENROUTER_API_KEY` | | `OPENROUTER_MODEL` | | `https://openrouter.ai/api/v1` | `openai/gpt-4o` |
| `groq` | `GROQ_API_KEY` | | `GROQ_MODEL` | | `https://api.groq.com/openai/v1` | `llama-3.3-70b-versatile` |
| `nvidia` | `NVIDIA_API_KEY` | | `NVIDIA_MODEL` | | `https://integrate.api.nvidia.com/v1` | `meta/llama-3.3-70b-instruct` |
| `venice` | `VENICE_API_KEY` | | `VENICE_MODEL` | | `https://api.venice.ai/api/v1` | `llama-3.3-70b` |
| `together` | `TOGETHER_API_KEY` | | `TOGETHER_MODEL` | | `https://api.together.xyz/v1` | `meta-llama/Llama-3-70b-chat-hf` |
| `fireworks` | `FIREWORKS_API_KEY` | | `FIREWORKS_MODEL` | | `https://api.fireworks.ai/inference/v1` | `accounts/fireworks/models/llama-v3p1-70b-instruct` |
| `deepseek` | `DEEPSEEK_API_KEY` | | `DEEPSEEK_MODEL` | | `https://api.deepseek.com/v1` | `deepseek-chat` |
| `cerebras` | `CEREBRAS_API_KEY` | | `CEREBRAS_MODEL` | | `https://api.cerebras.ai/v1` | `llama-3.3-70b` |
| `sambanova` | `SAMBANOVA_API_KEY` | | `SAMBANOVA_MODEL` | | `https://api.sambanova.ai/v1` | `Meta-Llama-3.1-70B-Instruct` |
| `gemini` | `GEMINI_API_KEY` | | `GEMINI_MODEL` | | `https://generativelanguage.googleapis.com/v1beta/openai` | `gemini-2.5-flash` |
| `ionet` | `IONET_API_KEY` | | `IONET_MODEL` | | `https://api.intelligence.io.solutions/api/v1` | `deepseek-coder-v2-instruct` |
| `mistral` | `MISTRAL_API_KEY` | | `MISTRAL_MODEL` | | `https://api.mistral.ai/v1` | `mistral-large-latest` |
| `yandex` | `YANDEX_API_KEY` | | `YANDEX_MODEL` | `YANDEX_EXTRA_HEADERS` | `https://ai.api.cloud.yandex.net/v1` | `yandexgpt-lite` |
| `cloudflare` | `CLOUDFLARE_API_KEY` | `CLOUDFLARE_BASE_URL` | `CLOUDFLARE_MODEL` | | provider default | `@cf/meta/llama-3.3-70b-instruct-fp8-fast` |

### 4.7 WASM, sandboxing, Claude Code, and extension-development overrides

Table 23. WASM, sandbox, and development-override environment variables.

| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `WASM_ENABLED` | Enable WASM tool execution. | Boolean, default `true`. |
| `WASM_TOOLS_DIR` | Directory containing installed WASM tools. | Defaults to `~/.ironclaw/tools`. |
| `WASM_DEFAULT_MEMORY_LIMIT` | Default WASM memory limit in bytes. | Defaults to `10485760` (10 MiB). |
| `WASM_DEFAULT_TIMEOUT_SECS` | Default WASM execution timeout. | Defaults to `60`. |
| `WASM_DEFAULT_FUEL_LIMIT` | Default WASM fuel budget. | Defaults to `10000000`. |
| `WASM_CACHE_COMPILED` | Cache compiled WASM modules. | Boolean, default `true`. |
| `WASM_CACHE_DIR` | Explicit path for the compiled-module cache. | Optional. |
| `SANDBOX_ENABLED` | Enable the Docker-backed sandbox. | Boolean, default `true`. |
| `SANDBOX_POLICY` | Filesystem policy for sandbox jobs. | Intended values are `readonly`, `workspace_write`, and `full_access`; unknown values currently fall back to `readonly` at runtime. |
| `SANDBOX_TIMEOUT_SECS` | Sandbox command timeout. | Defaults to `120`. |
| `SANDBOX_MEMORY_LIMIT_MB` | Sandbox memory limit in MiB. | Defaults to `2048`. |
| `SANDBOX_CPU_SHARES` | Relative CPU share weight. | Defaults to `1024`. |
| `SANDBOX_IMAGE` | Docker image used for sandbox workers. | Defaults to `ironclaw-worker:latest`. |
| `SANDBOX_AUTO_PULL` | Auto-pull the sandbox image when missing. | Boolean, default `true`. |
| `SANDBOX_EXTRA_DOMAINS` | Comma-separated extra network allowlist entries. | Optional. |
| `SANDBOX_REAPER_INTERVAL_SECS` | Reaper scan interval. | Defaults to `300`, and must be greater than `0`. |
| `SANDBOX_ORPHAN_THRESHOLD_SECS` | Age threshold for orphaned-container reaping. | Defaults to `600`, and must be greater than `0`. |
| `CLAUDE_CODE_ENABLED` | Expose Claude Code sandbox mode. | Boolean, default `false`. |
| `CLAUDE_CONFIG_DIR` | Host directory containing Claude configuration. | Defaults to `~/.claude`. |
| `CLAUDE_CODE_MODEL` | Claude Code model selector. | Defaults to `sonnet`. |
| `CLAUDE_CODE_MAX_TURNS` | Maximum turns for Claude Code runs. | Defaults to `50`. |
| `CLAUDE_CODE_MEMORY_LIMIT_MB` | Memory limit for Claude Code containers. | Defaults to `4096`. |
| `CLAUDE_CODE_ALLOWED_TOOLS` | Comma-separated allowlist for Claude Code auto-approved tools. | Defaults to a built-in list covering read, write, edit, bash, task, and web tools. |
| `IRONCLAW_TOOLS_SRC` | Override the development `tools-src/` directory. | Intended for development or packaging flows, not normal runtime configuration. |
| `IRONCLAW_CHANNELS_SRC` | Override the development `channels-src/` directory. | Intended for development or packaging flows, not normal runtime configuration. |
| `CARGO_TARGET_DIR` | Override the shared Cargo target directory used when locating dev-built WASM artefacts. | Development-only. |
| `CLAWHUB_REGISTRY` | Override the skill-catalog registry base URL. | Development or staging override. |
| `CLAWDHUB_REGISTRY` | Legacy fallback for `CLAWHUB_REGISTRY`. | Kept for backward compatibility. |

### 4.8 OAuth, tracing, relay quirks, and internal-only switches

Table 24. Advanced, internal, and debug-oriented environment variables.

| Variable | Meaning | Default or rule |
|----------|---------|-----------------|
| `IRONCLAW_OAUTH_EXCHANGE_URL` | Proxy URL used by gateway OAuth completion to exchange the authorization code through another service. | Optional. When unset, the gateway exchanges directly with the upstream token endpoint. |
| `OAUTH_CALLBACK_HOST` | Host used by the local OAuth callback listener. | Defaults to `127.0.0.1`. Wildcard addresses such as `0.0.0.0` and `::` are rejected. |
| `IRONCLAW_INSTANCE_NAME` | Prefix applied to OAuth CSRF state for platform routing. | Optional. |
| `OPENCLAW_INSTANCE_NAME` | Legacy fallback for `IRONCLAW_INSTANCE_NAME`. | Optional, kept for backwards compatibility. |
| `IRONCLAW_USER_ID` | Override the relay-auth user UUID derivation. | Optional. |
| `IRONCLAW_RECORD_TRACE` | Enable LLM trace recording when set to any non-empty value. | Disabled when unset or empty. |
| `IRONCLAW_TRACE_OUTPUT` | Output file path for recorded traces. | Defaults to `./trace_<timestamp>.json`. |
| `IRONCLAW_TRACE_MODEL_NAME` | Override the model name stored in a trace file. | Defaults to `recorded-<inner-model-name>`. |
| `IRONCLAW_IN_DOCKER` | Declare that the host is running inside the Docker restart environment. | Must be `true` for the restart tool to perform a process exit. |
| `IRONCLAW_DISABLE_RESTART` | Suppress the actual process exit in the restart tool. | Intended for tests or controlled development runs. |
| `IRONCLAW_WORKER_TOKEN` | Bearer token used by internal sandbox workers when talking to the orchestrator. | Required for `worker` and `claude-bridge` container entrypoints. |
| `CLAUDE_CODE_OAUTH_TOKEN` | OAuth token discovered by the Claude Code bridge when no API key is present. | Intended for container and bridge flows rather than the main config loader. |
| `IRONCLAW_PID_LOCK_CHILD` | Internal test knob for PID-lock child processes. | Not intended for user-facing configuration. |
| `IRONCLAW_PID_LOCK_PATH` | Internal PID-lock test path override. | Not intended for user-facing configuration. |
| `IRONCLAW_PID_LOCK_HOLD_MS` | Internal PID-lock test hold duration. | Not intended for user-facing configuration. |
| `IRONCLAW_E2E_DOCKER_TESTS` | Enable Docker-backed end-to-end tests for the reaper. | Test-only. |
| `RUST_LOG` | Standard `tracing` filter used by the web log layer. | Defaults internally to `ironclaw=info,tower_http=warn` when the variable is unset. |

The runtime also consults platform variables such as `HOME`,
`XDG_RUNTIME_DIR`, and `UID` for home-directory resolution and rootless Docker
socket discovery. Those are normal operating-system inputs rather than
axinite-specific configuration knobs, but they still affect startup behaviour
in container-heavy environments.

## 5. Validation rules and operational gotchas

Several settings have behaviour that is easy to miss if the guide only lists
names.

1. Boolean parsing is strict in the shared config helpers. For most settings,
   only `true`, `false`, `1`, and `0` are accepted. A few older call sites,
   such as parts of the Signal config and tunnel config, still do their own
   looser parsing.
1. `HTTP_HOST` and `HTTP_PORT` do not merely change values on an always-on HTTP
   listener. The listener is created only when one of those variables is set.
1. `HTTP_WEBHOOK_SECRET` secures webhook traffic, but setting it alone does
   not start the HTTP listener. One of `HTTP_HOST` or `HTTP_PORT` must also be
   set.
1. `GATEWAY_ENABLED=false` disables the web gateway entirely, even if
   `GATEWAY_HOST` and `GATEWAY_PORT` are also set.
1. `LIBSQL_URL` requires `LIBSQL_AUTH_TOKEN`. Setting only the URL is treated
   as an error.
1. `BEDROCK_MODEL` is mandatory when `LLM_BACKEND=bedrock`.
1. `DEFAULT_TIMEZONE` and `HEARTBEAT_TIMEZONE` must be valid IANA timezone
   names.
1. `TUNNEL_URL` must start with `https://`, because the webhook-capable
   channels assume HTTPS.
1. The relay OAuth path has one implementation quirk worth knowing about. Most
   gateway code defaults to port `3000`, but one fallback path in the relay
   auth flow reconstructs the callback base from `GATEWAY_HOST` and
   `GATEWAY_PORT` and falls back to port `3001` when `GATEWAY_PORT` is unset.
   If relay OAuth callbacks look wrong, check that pair first.
1. Unknown `LLM_BACKEND` values do not fail closed. The runtime warns, then
   tries to interpret the backend through the generic OpenAI-compatible
   provider shape.
1. The built-in provider table is not exhaustive once a local
   `~/.ironclaw/providers.json` exists. Custom provider definitions can add new
   provider IDs and entirely different environment-variable names.
