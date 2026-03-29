# Axinite

*A security-minded personal AI assistant forked from IronClaw, built to keep
your data local and your tooling under your control.*

![Axinite](https://raw.githubusercontent.com/leynos/axinite/refs/heads/staging/axinite-mascot-smol.jpg)

[English](README.md) | [简体中文](README.zh-CN.md)

Axinite inherits a large share of IronClaw's runtime, CLI surface, and
documentation. That heritage is deliberate: this fork keeps the upstream
security and automation machinery while taking the project in its own
direction. For the moment, the compiled binary, crate name, and many internal
paths still use `ironclaw`.

______________________________________________________________________

## Why axinite?

- **Local-first by default**: data, secrets, and audit trails stay on your
  machine rather than wandering off into somebody else's service.
- **Security is a core feature**: sandboxed WASM tools, host-side secret
  injection, leak detection, and endpoint allowlists are part of the runtime.
- **It is more than a chat box**: REPL, web gateway, webhooks, routines,
  background jobs, and channel integrations all live in one assistant.
- **It can grow with you**: install MCP servers, add WASM tools, and extend the
  agent without waiting for a vendor to ship the exact feature you need.

______________________________________________________________________

## Quick start

### Installation

Axinite is the fork name. The current crate, binary, and setup command are
still called `ironclaw`.

```bash
cargo build
target/debug/ironclaw onboard --quick
```

### Basic usage

```bash
# Send one message and exit
target/debug/ironclaw --message "Summarize what this machine is ready to do."

# Check health and configured services
target/debug/ironclaw status

# Inspect workspace memory support
target/debug/ironclaw memory status
```

______________________________________________________________________

## Features

- Secure runtime with WASM sandboxing, prompt sanitization, credential
  protection, and network allowlisting.
- Multiple interfaces: interactive CLI, single-message mode, web gateway,
  webhooks, routines, and OS service support.
- Persistent workspace memory with hybrid search over notes, logs, and identity
  files.
- Extensible toolchain with built-in tools, MCP servers, registry-managed
  extensions, and dynamically built WASM tools.
- Flexible provider story with onboarding support for NEAR AI, OpenAI-compatible
  endpoints, Ollama, Bedrock, and more.

______________________________________________________________________

## Learn more

- [LLM provider guide](docs/LLM_PROVIDERS.md) — provider setup and environment
  variables.
- [Onboarding spec](src/setup/README.md) — what `ironclaw onboard` configures.
- [Workspace and memory](src/workspace/README.md) — persistent memory layout
  and tools.
- [Building channels](docs/BUILDING_CHANNELS.md) — rebuilding bundled channel
  artefacts.
- [Contributing](CONTRIBUTING.md) — development workflow and review tracks.
- [Changelog](CHANGELOG.md) — release history.

______________________________________________________________________

## Licence

Dual-licensed under MIT or Apache-2.0. See [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE) for details.

______________________________________________________________________

## Contributing

Contributions are welcome. Please read [AGENTS.md](AGENTS.md) and
[CONTRIBUTING.md](CONTRIBUTING.md) before you start; this repository expects
gated commits, explicit review tracks, and honest status reporting.
