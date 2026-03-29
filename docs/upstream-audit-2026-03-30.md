<!-- markdownlint-disable MD013 MD024 -->

# Upstream staging audit for 2026-03-30

## Scope

- Audited `upstream/staging` commits after `edca67e8b1f31525aed4eb35f72db373547cee64` through `368d2f523868cc06a3a84fc1255dd7411f680da6`.
- Method: `git log`, `git show --shortstat`, targeted `sem diff` on the highest-impact changes, and comparison against Axinite's mission and roadmap in `docs/welcome-to-axinite.md`, `docs/roadmap.md`, and `docs/axinite-architecture-overview.md`.
- Reading rule: fixes record whether `main` likely needs follow-up; features record architectural compatibility, scope/size, and goal fit; chores and supporting changes note whether the fork needs to act.

## Executive summary

- Highest-priority upstream fixes to assess for `main`: DOMPurify-based web sanitization, DNS rebinding/SSRF hardening, header-based HMAC webhook auth plus CSP and fail-closed secret handling, the broad March 13 runtime bug sweep, the MCP audit-finding sweep, canonical typed WASM schema advertisement, and the injection-safe PTY worker change.
- Best-aligned upstream features: localized web UI, recursive skill discovery as a stepping stone toward multi-file skill bundles, Telegram improvements, approval-risk tiering, and selective web-gateway UX work.
- Useful references but not direct carries: the OpenAI Responses API gateway endpoints and unified settings UI. They overlap Axinite goals, but upstream lands them at different seams than the fork's planned provider-first and information-architecture design.
- Clear non-fits for Axinite `main`: the multi-tenant auth and user-management stream, reverse-proxy OIDC gateway auth, and most provider-sprawl or non-Telegram channel-expansion work.
- Large upstream memory changes should be treated carefully: privacy-aware layered workspace memory is conceptually adjacent, but it conflicts with the fork's `memoryd` sidecar direction.

## Full ledger

### 2026-03-11

- `369741fc60bf4ec1a28445c23d99db4a7f9c04c3` [feat] Add generic host-verified /webhook/tools/{tool} ingress. Arch: partial. Size: large. Mission: mixed. Generic tool webhooks fit the existing unified webhook server, but the feature is broader than Axinite's current fork goals and would need review against provenance-aware intent enforcement before adoption.
- `8f513428f1ee7e7321b2c0c25446d1edc3839072` [fix] Resolve deferred review items from PRs #883, #848, #788. Main: maybe. Severity: medium. Effectiveness: follow-up. Scope/blast: broad web gateway, src.
- `6b841bb8170ff52f576c4a707160d1a834e98f12` [feat] Add internationalization support with Chinese and English translations. Arch: direct. Size: medium. Mission: strong. Localized web UI is explicitly in scope for Axinite; this is a good upstream feature to mine, although the fork should keep terminology and branding aligned with its own docs.
- `6a1301bc5bcf5cda9055fb4d4e77acf02943381c` [feat] Add internationalization support with Chinese and English translations (#929). Arch: direct. Size: medium. Mission: strong. This is a follow-up promotion of the same i18n feature and remains compatible with Axinite's localized UI goal.
- `8391415bcef0b322c858cd6be62582d0d2e2bd6e` [chore] Update WASM artifact SHA256 checksums [skip ci]. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `fe82469904a797c9e4b87998a47c00a3daa16f29` [fix] WASM WIT compat sqlite3 duplicate symbol conflict. Main: yes. Severity: low. Effectiveness: targeted. Scope/blast: very broad Cargo.lock, Cargo.toml.
- `34550add3ee85bab6fe1c670dff46871d1a41e04` [fix] Prevent staging-ci tag failure and chained PR auto-close. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: narrow CI/release.
- `f08220db8201013acffff05898b261d81f7f0f5e` [fix] Run gated test jobs during staging CI. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: narrow CI/release.
- `d313f44a1977a52af023abfdfc52a378fed2c8f0` [fix] Improve Claude Code review reliability. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: narrow CI/release.
- `ffbc0cd1d4d6c84c70faacb2391ec5160782c205` [merge] Merge pull request #962 from nearai/staging-promote/d313f44a-22974575035. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `696d6a0bc86d620c0171ad3647226fc0d5053be0` [merge] Merge pull request #957 from nearai/staging-promote/34550add-22970193833. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `99dadcb0eab791a9a18913bbd01a7bbd6af12c0d` [merge] Merge pull request #925 from nearai/staging-promote/8f513428-22941325130. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `28a22f2a59239df9ea6efd7430a9491b960471c5` [fix] Replace regex HTML sanitizer with DOMPurify to prevent XSS. Main: yes. Severity: critical. Effectiveness: strong. Scope/blast: narrow web gateway HTML rendering path. Replaces a regex sanitizer with DOMPurify and closes a prompt-injection-to-XSS chain in the browser UI.
- `19d9562b4f546b566129ba4d781b47c2f164872d` [feat] Unify auth and configure into single entrypoint. Arch: partial. Size: large. Mission: mixed. Scope: channels-src, agent runtime.
- `bb0656577091ee9e10611f583fa9572d85dccf83` [fix] Resolve DNS once and reuse for SSRF validation to prevent rebinding. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow HTTP/network boundary. DNS result pinning is directly relevant to Axinite's MCP-over-HTTPS and delegated-endpoint hardening story.
- `94b448ffab521d82ff8226e8eb26887b6ed00155` [fix] Load WASM tool description and schema from capabilities.json. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: moderate WASM tools.
- `d7024f557ff3136d0cfe0621f963c5a7a262759a` [merge] Merge pull request #917 from nearai/staging-promote/369741fc-22935740447. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.

### 2026-03-12

- `6321bb46883fb5fd51fb8236b3e3f427a66586ce` [fix] Drain residual events and filter key kind in onboard prompts (#937). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.
- `c8cac0925dbb2ee7d3eea573cce985136a17ce31` [fix] Stdio/unix transports skip initialize handshake (#890). Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate src.
- `2094d6e30d6fe7330b423d0bbc00fbe759722c76` [fix] Block thread_id-based context pollution across users. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow session isolation path. Cross-user context pollution is upstream multi-user framing, but the underlying thread/session boundary bug is worth checking anywhere shared session state survives.
- `a1b3911b27ac20daae1eb2e3e1fd7d5f6b35f548` [fix] Header safety validation and Authorization conflict bug from #704. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate src.
- `5879d06447726a5bfe61566d81d347e2ca3d3be3` [fix] Drain tunnel pipes to prevent zombie process. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate src.
- `d47282f44424486b4d74a34cd22592b440f91140` [fix] Validate channel credentials during setup. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.
- `f3e8e7c599dccfda0e4328e0385346a61bfdd4e0` [docs] Add Russian localization (README.ru.md). Need main: maybe. Docs note: moderate change across docs; only worth porting if the same surface is active in Axinite.
- `977b7fde996d3b417b10ab73fbeb3159808257ac` [feat] Display ASCII art banner during onboarding. Arch: partial. Size: small. Mission: mixed. Scope: src.

### 2026-03-11

- `81f7b64994051b7ae148cce7cbadf1973f204d92` [fix] Disambiguate WASM bundle filenames to prevent tool/channel collision. Main: yes. Severity: low. Effectiveness: targeted. Scope/blast: broad CI/release, scripts.

### 2026-03-12

- `c372c9972983e8d48bc97ab358633958a27202b7` [fix] Stabilize openai compat oversized-body regression. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: moderate web gateway, tests.
- `acea1143cf70f7fa593c077620c979d5aa260de9` [fix] Fix systemctl unit. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.

### 2026-03-11

- `f48fe95ac41e916a67bcc1482a9ce6450425452d` [fix] Add Content-Security-Policy header to web gateway. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow web gateway. CSP is an obvious defence-in-depth carry-forward for Axinite's browser UI.
- `8bbb43da52c3503833ceb30fc5c633175f672010` [fix] Require explicit SANDBOX_ALLOW_FULL_ACCESS to enable FullAccess policy. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow sandbox policy surface. Making full-access execution opt-in matches Axinite's security-first posture.
- `a9821ac20f0509bcfdfef8c5f2ed95b40ca4ec05` [fix] Make unsafe env::set_var calls safe with explicit invariants. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: broad src, config.
- `195ff44b1a653750ce3c10dd2214200df097ec0b` [fix] Migrate webhook auth to HMAC-SHA256 signature header. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow webhook auth path. Header-based HMAC signing is materially safer than body-secret transport and aligns with Axinite's fail-closed ingress stance.
- `f31cd13135ab75db1ae7011c01ae0da6431048c5` [fix] Attach session manager for non-OAuth HTTP clients (#793). Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow src.
- `c37b64124c3f2342957c02255303124bb2cc6c35` [fix] Preserve model selection on provider re-run (#679). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.

### 2026-03-12

- `febed1e12ef03029c4627ff0fa2a7dcb59309a20` [feat] Add cargo-deny for supply chain safety. Arch: direct. Size: small. Mission: strong. Supply-chain denial fits the security-first fork stance and is worth keeping on the radar if Axinite has not already adopted an equivalent gate.
- `f05896fe6a0ffaccb61f3a0ab4f3f9128e6df6fd` [support] Migrate GitHub webhook normalization into github tool. Need main: maybe. Supporting note: useful only if Axinite keeps upstream's GitHub tool packaging and webhook flow; otherwise this is implementation churn, not a fork priority.
- `3fbe290901a004d4c7de5ee1182cf4ada63a9127` [feat] Add `ironclaw skills list/search/info` subcommands. Arch: direct. Size: medium. Mission: strong. Scope: parity docs, src.

### 2026-03-11

- `269b3f462f5e4bce0fc0cc9a97d97e02b78de905` [test] Refresh golden files after renderer bump. Need main: maybe. Test note: narrow change across tests; only worth porting if the same surface is active in Axinite.

### 2026-03-12

- `5d9d17bf712cba8431150cc6cd7bc6be7cccf826` [feat] Add `ironclaw channels list` subcommand. Arch: partial. Size: medium. Mission: mixed. Scope: parity docs, src.
- `e2eb340c049c02e860c53e94e9631c5cf3f397ed` [feat] Add Z.AI provider support for GLM-5. Arch: low. Size: medium. Mission: weak. Z.AI support broadens provider coverage without advancing a core fork goal.
- `5a62ceaa99043d87312a6ec0c59d8910ca5b2e97` [refactor] Extract safety module into ironclaw_safety crate. Need main: maybe. Refactor note: very broad change across CLAUDE.md, Cargo.lock; only worth porting if the same surface is active in Axinite.
- `c937dfa315d84017f8b8c01dc1e534a855f1c2a3` [fix] Use versioned artifact URLs and checksums for all WASM manifests. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: very broad registry.
- `ef34943c14993d4db155d7f6ea07650266732e05` [fix] Release lock guards before awaiting channel send (#869). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: very broad CI/release, webhooks.
- `c26f116a987961885a8193022539bef9b2d3ec13` [fix] Harden production container and bootstrap security. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: moderate deploy.
- `0b81342b5cd1e0948d73a8f6582d7ea0098be0d7` [fix] Fix UTF-8 unsafe truncation in WASM emit_message. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow WASM channels.
- `8a26cfae736526dc13aed47dd17f53ca135c496e` [fix] Open MCP OAuth in same browser as gateway. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad .githooks, agent runtime.
- `4faf81ab612eeecb2f955416ea205b7c91b95867` [fix] Include OAuth state parameter in authorization URLs. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow src.
- `f776d96395c1b78db86a7b4704b5861c78dacab0` [fix] Remove all inline event handlers for CSP script-src compliance. Main: maybe. Severity: high. Effectiveness: targeted. Scope/blast: very broad CI/release, CHANGELOG.md.

### 2026-03-13

- `863702a87a0f0f02248bb9a2534673cc4b1fcfd9` [feat] Add MiniMax as a built-in LLM provider. Arch: low. Size: medium. Mission: weak. Another built-in LLM provider expands breadth more than depth and is not part of Axinite's current narrow-surface story.

### 2026-03-12

- `d420abfa6ac1a80f7ebd426913d503447b66786e` [fix] Reject absolute filesystem paths with corrective routing. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate CI/release, tool runtime.
- `006c15e79c31f5025b9ce2e8ec57f3a8fd829f5a` [style] Remove unnecessary Worker re-export. Need main: maybe. Style note: narrow change across agent runtime; only worth porting if the same surface is active in Axinite.

### 2026-03-13

- `6bbf87ba3aff78141560cbfb4a2ec310ae1561d3` [feat] Enable tool access in lightweight routine execution (#257). Arch: partial. Size: medium. Mission: mixed. Scope: agent runtime, src.
- `8df51c04ae8cfa8fbf71240e9aaf23c70f34fbca` [feat] Enhance HTTP tool parameter parsing. Arch: partial. Size: small. Mission: mixed. Scope: tool runtime.

### 2026-03-12

- `c94ecf19db59f8c70c488f0a761c5f4cbf674a1a` [feat] Add Slack approval buttons for tool execution in DMs. Arch: low. Size: small. Mission: weak. Scope: agent runtime, relay.
- `0b122cb28f93eb8b5f1baea3ada2dd46d3a010d9` [feat] Add hover copy button for user/assistant messages. Arch: partial. Size: small. Mission: mixed. Scope: web gateway.
- `c592c50dad6f7f8a54667b8016828ca48262cae0` [discord] Mentions + signature verification in WASM channel. Need main: maybe. Supporting change touching channels-src; evaluate only if dependent upstream fixes/features are adopted.
- `fd574b2859023665a08773a2cebc9d9242cc6ddd` [feat] Expose the shared agent session manager via AppComponents. Arch: partial. Size: small. Mission: mixed. Scope: src.
- `5dfa66669108b8fc4c7d0cb1b80a8c533bb1948b` [feat] Adds context-llm tool support. Arch: partial. Size: medium. Mission: mixed. Scope: registry, tools-src.
- `bcda73c2e05264813d09d21e144e54d1c306e11f` [feat] Add cron subcommand for managing scheduled routines. Arch: partial. Size: large. Mission: mixed. Scope: parity docs, src.
- `8ac24e775b2eacf482f60d144bf513eb0a759fe0` [style] Fix formatting in cli/mod.rs and mcp/auth.rs. Need main: maybe. Style note: moderate change across src, tool runtime; only worth porting if the same surface is active in Axinite.
- `e1691a8d429e178d5708f60200fad43e4786e93f` [feat] Configurable hybrid search fusion strategy. Arch: low. Size: medium. Mission: weak. Upstream hybrid-search tuning sits on the in-process memory design Axinite plans to replace with memoryd, so this is not a direct feature carry.
- `d5828b271dfac7b3c23ee678f7aceccfd1d2fd78` [feat] Add reusable sensitive JSON redaction helper. Arch: partial. Size: small. Mission: mixed. Scope: src.
- `442a42d996fbb91a2924e247e9a679f9585968f4` [fix] Recompute cron next_fire_at when re-enabling routines. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate web gateway, tests.
- `7a9cbb3b504c82eb6456b20b1c339734fc2f93f2` [fix] Run cron checks immediately on ticker startup. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate agent runtime, tool runtime.
- `e522d33a53866ab62327bb70002930e9509182a2` [fix] Make approval requests appear without page reload (#996). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow web gateway.
- `6f004909007035f0ec368e92908f87c45f495e16` [fix] Relax approval requirements for low-risk tools. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: moderate tool runtime.
- `d8bcfe15cf54be18fc36b60feb1582e8d6a5a962` [fix] Set CLI_ENABLED=false in macOS launchd plist. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.
- `1ba6a83ca4c939514700dc099c882524a9108de9` [fix] Fail closed when webhook secret is missing at runtime. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow webhooks.

### 2026-03-13

- `8c2131db48bafaf4422d21b09cd37dfc150bb13c` [feat] Add EMBEDDING_BASE_URL for OpenAI-compatible embedding providers. Arch: partial. Size: small. Mission: mixed. Scope: config, workspace/memory.

### 2026-03-12

- `c54f739354150f68414fc923ce42e6683ba7967b` [fix] Resolve bug_bash UX/logging issues (#1054 #1055 #1058). Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: broad web gateway, database.
- `c7dec64b2dd4f94a0c1f39a37c6c0b3cbbb9646b` [feat] Include commit history in staging promotion PRs. Arch: partial. Size: small. Mission: mixed. Scope: CI/release.
- `a71a5038705aea9966b790be82debb5020d3442b` [merge] Merge pull request #1065 from nearai/staging-promote/f776d963-23017191214. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `8a60fa2d37793e27d797b4438feec81e8ed8330a` [fix] Add tool_info schema discovery for WASM tools. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad src, tool runtime.
- `9fbdd4298855e60d1e661a6f59e07f231c14693b` [fix] Fix lifecycle bugs + comprehensive E2E tests. Main: maybe. Severity: medium-high. Effectiveness: comprehensive. Scope/blast: very broad CI/release, web gateway.
- `cd1245afc099277d6f3f20a454cd0ca4e9edf3eb` [fix] Repair staging-ci workflow parsing. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: narrow CI/release.
- `15c5d3e2e2f4a3ddeb0ee7a35bcdba2605e0b1a4` [fix] Address #1086 review followups -- description hint and coercion safety. Main: yes. Severity: low. Effectiveness: targeted. Scope/blast: narrow WASM tools.
- `3c619b627297d042d52fd87c915d31284e7df907` [fix] Repair staging promotion workflow behavior. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: broad CI/release.
- `a89cf379938b1fdc58a6ecb11233f5ae90e786eb` [fix] Bump telegram channel version for capabilities change. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow registry.
- `c47237b9c7c89d2570b4b788dba7e3c5ee70a59b` [fix] Add missing attachments field and crates/ dir to Dockerfiles. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: broad Dockerfile, Dockerfile.test.
- `5e7758598fb858dc48bc08cdb57da674a31b4339` [chore] Periodic sync main into staging (resolved conflicts). Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `1e00b1fed50ac88f78d128e4bd4e9243cecdae3e` [fix] Checkout promotion PR head for metadata refresh. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: narrow CI/release.
- `3149c9111615efcc4cf369be0d4bf3331968dc5f` [merge] Merge pull request #1102 from nearai/staging-promote/1e00b1fe-23036363919. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `2b8063a8cf7e54eb741e0e6eb9af7950d109dcc7` [merge] Merge pull request #1096 from nearai/staging-promote/3c619b62-23035039465. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `a3c99f28017526e267ac0ae20577d774a2e9a1fd` [merge] Merge branch 'main' into staging-promote/e2eb340c-22999151534. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `ca6d9f6ede87795de769bf7acaebd45b514cfc83` [fix] Bump versions for github, web-search, and discord extensions. Main: unlikely. Severity: medium. Effectiveness: targeted. Scope/blast: moderate registry.
- `f470f5db8041f377aa7dc3eeeef6dec3fc1b4ab6` [merge] Merge pull request #1032 from nearai/staging-promote/e2eb340c-22999151534. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.

### 2026-03-13

- `e805ec61aa6e744679cebb73b86bfc5e26ca5e6f` [fix] 5 critical/high-priority bugs (auth bypass, relay failures, unbounded recursion, context growth). Main: yes. Severity: high. Effectiveness: comprehensive. Scope/blast: broad but still targeted at core runtime guardrails. The depth limits, runtime auth checks, context truncation, and SSRF hardening all hit surfaces Axinite still carries.
- `7776d267f8f8e8468e62953abc2144ccf9337a11` [ci] Enforce no .unwrap(), .expect(), or assert!() in production code. Need main: maybe. Ci note: moderate change across CI/release, scripts; only worth porting if the same surface is active in Axinite.

### 2026-03-14

- `275bcfb65866a25334909393ebeaa2ed32827055` [fix] Treat empty timezone string as absent. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow tool runtime.

### 2026-03-13

- `bc6725205ada24f26ed30fd042dc4aa6b546cb93` [fix] Replace .expect() with match in webhook handler. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow webhooks.
- `f53c1bb10beba3f6bb1f127c34371a6c0bf6f510` [fix] Address 14 audit findings across MCP module. Main: yes. Severity: high. Effectiveness: comprehensive. Scope/blast: medium MCP/runtime core. A 14-finding MCP sweep is directly relevant because MCP over HTTPS is central to Axinite's mission.
- `1bc10fe4ca7f085d86e8cfa45ca234de7002e95b` [test] Add event-trigger routine e2e coverage. Need main: maybe. Test note: moderate change across tests; only worth porting if the same surface is active in Axinite.
- `7d745d5479387a3e5de4f4e6a19c20ed23f5f713` [tools] Improve routine schema guidance. Need main: maybe. Supporting change touching tool runtime, src; evaluate only if dependent upstream fixes/features are adopted.
- `2b625ef3df968683e51c4f7a659fa4d7a1ba5b02` [fix] Bump versions for github, web-search, and discord extensions. Main: unlikely. Severity: medium. Effectiveness: targeted. Scope/blast: moderate registry.
- `3debe41f7109d9d94ec7e4339a731ff00792e3f3` [merge] Merge pull request #1149 from nearai/staging-promote/2b625ef3-23068472433. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `f9b880c2e99a9ef31e1f2853400f301c93a793e0` [fix] Exclude ironclaw_safety from release automation. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: moderate crates, release-plz.toml.

### 2026-03-14

- `757d24bd909d233d792f6271342e29c3ca58aa14` [feat] Add follow-up suggestion chips and ghost text. Arch: partial. Size: medium. Mission: mixed. Suggestion chips improve the gateway UX, but they are additive polish rather than core fork direction.
- `c916069dd236832c38a2a032ea8c3e578fe5585e` [refactor] Move MCP servers from code to JSON manifests. Arch: direct. Size: medium. Mission: mixed. Moving MCP server definitions into manifests simplifies extension metadata management and could support Axinite's catalogue work, but it is infrastructure, not a user-facing differentiator.
- `8fb2f70258e3dfcd8d16cc29c57e7d80c0734adf` [fix] HTTP webhook secret transmitted in request body rather than via header, docs inconsistency and security concern. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow webhook auth/docs surface. Carry this if Axinite still transmits webhook secrets in request bodies anywhere.
- `17706632794fe90674bad01cef9dad89a15fd10a` [fix] Google Sheets returns 403 PERMISSION_DENIED after completing OAuth. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad CI/release, WASM tools.
- `579c4fdbcabf1cbd5ce5f48764ca9b54bb81867f` [chore] Remove __pycache__ from repo and add to .gitignore. Need main: no. Chore note: repository maintenance with no obvious fork-specific action.
- `7c017ea6fd44a4a156013c39068ba1c690b1725b` [chore] Align manifest versions with published artifacts. Need main: no. Chore note: repository maintenance with no obvious fork-specific action.
- `8dfad332d96137bdcf3bafa265cb56f1111f052a` [fix] Avoid lock-held awaits in server lifecycle paths. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate web gateway, src.
- `3f2796b7453137a1e0de5b450a0574a521a68614` [fix] Non-transactional multi-step context updates between metadata/to…. Main: maybe. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate agent runtime, src.
- `5f0ed66a6ba6838fd4f9b057e7346a4a6467aac3` [perf] Avoid full message history clone each tool iteration. Need main: maybe. Perf note: moderate change across Cargo.lock, agent runtime; only worth porting if the same surface is active in Axinite.
- `cc52a046c1db34d388049e7f80c06b12e465675c` [fix] Use live owner binding during wasm hot activation. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: narrow extensions/registry.
- `ffe384b66ea326d58056cd6315b50fefa7c6beee` [fix] Add stop_sequences parity for tool completions. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: broad web gateway, LLM stack.
- `994a0b194fd3b59db9daa3e3b75ade71940205bd` [fix] N+1 query pattern in event trigger loop (routine_engine). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: very broad agent runtime, database.
- `e291d3b6f1eb1cb2b8f825586a7465ad422444d7` [feat] Human-readable cron schedule summaries in web UI. Arch: partial. Size: medium. Mission: mixed. Scope: agent runtime, web gateway.
- `71b1a6778b93a77a58b3278296a0bb0d8c2bb561` [fix] Update yanked uds_windows 1.2.0 -> 1.2.1. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow repo plumbing.
- `8753c482334b72dd97773b1d7c0e9ffbcba4c77f` [perf] Avoid reallocating SSE buffer on each chunk. Need main: maybe. Perf note: narrow change across src; only worth porting if the same surface is active in Axinite.
- `fda5160940b5661dc85150dc1bbedd3e1ca6b8fc` [support] Make no-panics CI check test-aware. Need main: maybe. Supporting change touching CI/release, scripts; evaluate only if dependent upstream fixes/features are adopted.
- `c79754df2888ac7e2704d6cf4686b111eceee959` [fix] Fix schema-guided tool parameter coercion. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad agent runtime, src.

### 2026-03-15

- `716629809cb8d3695e8342c3ade39fb211494837` [fix] Eliminate panic paths in production code. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: very broad crates, agent runtime.
- `15ab156d62632e173d9a10933b775cece6ea66a5` [feat] Add Criterion benchmarks for safety layer hot paths. Arch: partial. Size: medium. Mission: mixed. Scope: CI/release, Cargo.lock.
- `97b11ffd10ef91fa6a1ce169510830a3a3ef813a` [feat] Add Feishu/Lark WASM channel plugin. Arch: low. Size: large. Mission: weak. Feishu/Lark channel work adds another channel before Axinite has finished going deep on Telegram and the web gateway.
- `67b2c08a7c2c9c5a3f26c3bd2909691538cdc292` [feat] Add `logs` command for gateway log access. Arch: partial. Size: large. Mission: mixed. Scope: parity docs, src.
- `27e21fdabe72bb02b2aab7689b074815d87696c1` [feat] Add pre-push git hook with delta lint mode. Arch: partial. Size: medium. Mission: mixed. Scope: .githooks, scripts.
- `62d16e69ac89762c7a53429406ee90340de02055` [fix] Handle 400 auth errors, clear auth mode after OAuth, trim tokens. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad scripts, agent runtime.
- `a70e58f44e653ea0452e8f5a5c73c3c20f13c2a8` [fix] Prevent Safari IME composition Enter from sending message. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow web gateway.
- `f059d5033155a84551d3bcad25268c956c50f0a4` [fix] Preserve AuthError type in oauth_http_client cache. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow src.
- `3f6d2ab6c2c7e47fe5b3c6761a491fd4cd54a5cc` [fix] Treat empty url param as absent when installing skills. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: narrow tool runtime.
- `dac420840d01784fb7ca42e655b9a62763933bb9` [fix] Normalize chat copy to plain text. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate web gateway, tests.
- `e74214dce8fe6013b8a9a8dd02fd13cacf263131` [fix] Unify ChannelsConfig resolution to env > settings > default. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: broad config, extensions/registry.
- `c4e098d4e3d693b3285425ec30baec72588fc80d` [fix] Fix subagent monitor events being treated as user input. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: broad Cargo.lock, agent runtime.
- `e0f393bf04ffc29d9de4108c6725b3380b83536b` [fix] Avoid false success and block chat during pending auth. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate agent runtime, web gateway.
- `6aaa89010a5bf766e90095024638cde1e39eaecf` [fix] Default webhook server to loopback when tunnel is configured. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: moderate src, config.

### 2026-03-16

- `df8bb077378795254e698e088c4009815b9fa489` [fix] Fix conflict. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate crates.
- `3f874e73affa2328fe6688e012344c49bbc71f26` [fix] Resolve compilation errors in Feishu/Lark WASM channel (#1200). Main: unlikely. Severity: medium. Effectiveness: targeted. Scope/blast: narrow channels-src.

### 2026-03-15

- `bde0b77a86f6118a9a15afa576f0d995f77cda8b` [fix] Prevent metadata spoofing of internal job monitor flag. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: very broad agent runtime, src.
- `57c397bd502ac5752008b20006f103d763655b25` [docs] Mention MiniMax as built-in provider in all READMEs. Need main: maybe. Docs note: moderate change across docs; only worth porting if the same surface is active in Axinite.

### 2026-03-16

- `e81fb7e5cb6a3fe9e599285bf97dd601b2b7fcc1` [refactor] Extract init logic from wizard into owning modules. Need main: maybe. Refactor note: very broad change across config, database; only worth porting if the same surface is active in Axinite.

### 2026-03-15

- `81724cad93d2eeb8aa632ee8b23ab1d43c99d0c2` [fix] Telegram bot token validation fails intermittently (HTTP 404). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: very broad CI/release, .gitignore.

### 2026-03-16

- `1b59eb6b392685dbfa84ee9785bc02d90c9298ae` [feat] Reuse Codex CLI OAuth tokens for ChatGPT backend LLM calls. Arch: low. Size: large. Mission: weak. Reusing Codex CLI OAuth tokens broadens provider coupling and cloud assumptions rather than advancing the fork's narrow capability goals.
- `3e0e35d1bcb3a52c3333452e631af71095f7d1b2` [docs] Document relay manager init order. Need main: maybe. Docs note: narrow change across extensions/registry; only worth porting if the same surface is active in Axinite.
- `f618166ad8b21f8214a1b19b87bb180f51e4bbef` [feat] Fire_at time-of-day scheduling with IANA timezone. Arch: partial. Size: medium. Mission: mixed. Scope: Cargo.lock, agent runtime.
- `58a3eb136689b1aa573415a05e78620633a6ced0` [fix] Prevent orphaned tool_results and fix parallel merging. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: moderate LLM stack, sandbox/worker.
- `9e41b8acea49f38b0414d3f7955f69e8e204a0e5` [fix] Persist refreshed Anthropic OAuth token after Keychain re-read. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow LLM stack.
- `596d17f04b2780cea26824f92a25904a5d97339f` [fix] Make completed->completed transition idempotent to prevent race errors. Main: maybe. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate src, sandbox/worker.
- `0c31da46e7e1bc7a81db2b5f6a839807b91c75d5` [feat] Add retry logic for transient container failures. Arch: partial. Size: small. Mission: mixed. Scope: sandbox/worker.
- `877f117096800349bc8d0bb053331720618e8d4e` [feat] Add Chat Completions API provider for audio transcription. Arch: low. Size: medium. Mission: weak. Scope: config, src.
- `0245c0f9e99eb4b5189fa9e77f034174c72c7e42` [feat] Read ORCHESTRATOR_PORT env var for configurable API port. Arch: partial. Size: small. Mission: mixed. Scope: sandbox/worker.
- `a3579729086a8f3b3e9e260ebf3e725b5c3b4e4a` [feat] Unify config resolution with Settings fallback (Phase 2, #1119). Arch: partial. Size: medium. Mission: mixed. Scope: config, src.
- `946c040fff27cde387de288371e1e6bb2c902289` [feat] Add forum topic support with thread routing. Arch: direct. Size: medium. Mission: strong. Telegram remains one of Axinite's explicit target surfaces, so topic/thread routing is a good upstream feature candidate.
- `de214c23e0b107f591d00f9e2d1aa9963230bdb0` [feat] Add LLM_CHEAP_MODEL for generic smart routing across all backends. Arch: partial. Size: medium. Mission: mixed. Scope: config, LLM stack.
- `fe53f6993f68185daa8df6c299d310b269ff89b8` [chore] Promote staging to staging-promote/57c397bd-23120362128 (2026-03-16 05:35 UTC). Need main: no. Chore note: repository maintenance with no obvious fork-specific action.
- `ccdce69309a3d2e733e3bd9c1f7c538a7bfe0c40` [merge] Merge pull request #1185 from nearai/staging-promote/71b1a677-23096345848. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `aa3fac3edc366b3314e2743bbac8090494f85606` [merge] Merge pull request #1182 from nearai/staging-promote/579c4fdb-23095333790. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `190c70cdbe1b1997a7016de9958db309b1b167dd` [merge] Merge pull request #1176 from nearai/staging-promote/17706632-23094430993. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `4277a5a33a173522d04ca1dfc1f5df7eaac2dbf8` [merge] Merge pull request #1159 from nearai/staging-promote/f9b880c2-23080458788. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `d1c1bc79c51e521867bbfc268881ba3902b60504` [merge] Merge pull request #1145 from nearai/staging-promote/7d745d54-23066609095. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `a580c1d75fee342eb8ac54981036528b20cfec75` [merge] Merge pull request #1137 from nearai/staging-promote/f53c1bb1-23064256940. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `4c7afdb0ca68e055522c137d49217d0552fbd632` [merge] Merge pull request #1134 from nearai/staging-promote/bc672520-23062088162. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `63a23550d6b485de6eb3b9a8aefeee47de569ddd` [feat] Verify telegram owner during hot activation. Arch: direct. Size: very large. Mission: strong. Scope: parity docs, WASM channels.
- `9aca6a1053599c5cd08f950e5d5a8bd2a6c67de3` [merge] Merge pull request #1186 from nearai/staging-promote/8753c482-23098316440. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `b8ddbeadb4dd95e9bda91e780d4bdaaa099e58f4` [merge] Merge pull request #1188 from nearai/staging-promote/c79754df-23099429381. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `4890e73a342965d5312c2b68b8e26372e8beca0d` [merge] Merge pull request #1132 from nearai/staging-promote/e805ec61-23059634819. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `971b4c2ef43872d87dfbcbecce2587761c1dd860` [fix] Web/CLI routine mutations do not refresh live event trigger cache. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate web gateway, tests.
- `218e8778b93e46ce2855d8cf03ca6c191af74329` [merge] Merge pull request #1192 from nearai/staging-promote/15ab156d-23103553911. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `f2587e1f447e04f20d4a5019902d671353a84246` [merge] Merge pull request #1193 from nearai/staging-promote/97b11ffd-23104193988. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `ea0fa7c2c51460d1c797450e995116618ef4a3d7` [merge] Merge pull request #1196 from nearai/staging-promote/e74214dc-23104855330. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `e212c0066d4722989448b8b0b3be5af01acb950b` [merge] Merge pull request #1246 from nearai/staging-promote/63a23550-23151342222. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `8ba8def60786eed4d0c1f5e59d2b50640efec97a` [merge] Merge pull request #1239 from nearai/staging-promote/946c040f-23134229055. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `409a2ab9c07e52337ba12f03b8d8b42bd81d342e` [merge] Merge pull request #1231 from nearai/staging-promote/57c397bd-23120362128. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `e39754690217fb92660579bc6a3ef4aa3aa5a682` [merge] Merge pull request #1212 from nearai/staging-promote/3f874e73-23119318963. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `878a67cdb6608527c1bf6ac412180fc1fb2e56bc` [support] Refactor owner scope across channels and fix default routing fallback. Need main: maybe. Supporting change touching parity docs, channels-src; evaluate only if dependent upstream fixes/features are adopted.
- `b50eddfe0a044740a77f5a6885471a9179d59aeb` [merge] Merge branch 'main' into fix/resolve-conflicts. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `fc18064be9e3d9c3ad474f9deccb91a70c06d3e9` [fix] Resolve merge conflict fallout and missing config fields. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate LLM stack, src.
- `e7ddd460392cf5bb06d36e18079b2e00caf14468` [merge] Merge pull request #1262 from nearai/fix/resolve-conflicts. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `026beb00f2910277a7118bf7b9835b1892dc857e` [fix] Cover staging CI all-features and routine batch regressions. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: broad CI/release, database.
- `2961e70da14fbe7fe2bcd17e7f9941551d008cf3` [merge] Merge pull request #1263 from nearai/staging-promote/026beb00-23168216794. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `cb5f9796aa0909538bfcc6756193ab55d3e9bd58` [merge] Merge pull request #1260 from nearai/staging-promote/878a67cd-23166116689. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `1f209db0faa8169e2e83dff5b700e30db1aead9f` [fix] Bump channel registry versions for promotion. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow registry.
- `ed0ed40dae74185605b15e60a00c78d4b1fe39bd` [ci] Isolate heavy integration tests. Need main: maybe. Ci note: very broad change across CI/release, Cargo.toml; only worth porting if the same surface is active in Axinite.
- `c6128f4e41b5bd43d69a4432a6050df4d675590a` [fix] Misleading UI message. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate agent runtime, tests.
- `47659e954518d68e3a20c09021ef90fc647b7929` [merge] Merge pull request #1268 from nearai/staging-promote/c6128f4e-23170341776. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `d3e392ac167db2d1d39a3bf19eef97a5afc2bf64` [merge] Merge pull request #1267 from nearai/staging-promote/1f209db0-23170138026. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `9065527761d17df2bdb20cbeed1d986a80773737` [fix] Jobs limit. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.
- `d0cb5f0ac5052a17ab9d833a40e43e2218c94dd1` [test] Fix approval waiting regression coverage. Need main: maybe. Test note: moderate change across tests; only worth porting if the same surface is active in Axinite.
- `4675e9618c2f35e803c76476197fbb1d85059f43` [fix] Fix Telegram auto-verify flow and routing. Main: maybe. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad parity docs, agent runtime.
- `0e7eb7f39029c01463ed010aa8188614c86e6ec1` [merge] Merge pull request #1279 from nearai/staging-promote/4675e961-23176922462. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `2b6404e8b2a3a1e2fd6ba8a52d0b3478f9523cc9` [merge] Merge pull request #1276 from nearai/staging-promote/90655277-23176260323. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `deee24c65bef43aef89bed101300b1a69896ea1f` [merge] Merge pull request #1197 from nearai/staging-promote/e0f393bf-23105705354. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `5c56032b888b436825e150853c88ca3ea4172dbc` [fix] Rate limiter returns retry after None instead of a duration. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate LLM stack, workspace/memory.
- `1ad1335fea49927e91c2b13e18b60d96b0b86861` [chore] Release v0.19.0. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `ef5715cb9675a01654faa498efe78857cfaaded4` [fix] Mark ironclaw_safety unpublished in release-plz. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow release-plz.toml.
- `2784cef4d797cc8a36791010829c178b768a32b1` [fix] Relax timing thresholds in policy adversarial tests (100ms -> 500ms). Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: narrow crates.
- `059fd97ce65e6dad82ad44c0b59b17abecefda28` [merge] Merge pull request #1296 from nearai/staging-promote/2784cef4-23180012288. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `7a4673c11eaa4833223d8a41bf84d965cc83ddbe` [chore] Update WASM artifact SHA256 checksums [skip ci]. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.

### 2026-03-17

- `9bb05d2dcd387b36175a00b5803ff887854b40da` [merge] Merge pull request #1285 from nearai/staging-promote/5c56032b-23178585631. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.

### 2026-03-12

- `02fa404a9931522b0430e82699abe8a3f18f40a4` [fix] Add musl targets for Linux installer fallback. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: narrow Cargo.toml.

### 2026-03-13

- `bca8bbc8edf621fa63437e75123d2a637c8bb829` [fix] Update Cargo.lock and pin musl CI runners. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: moderate Cargo.lock, Cargo.toml.

### 2026-03-18

- `428303af1128e7f124ad623fc1338393a4d06fcc` [feat] Redesign routine create requests for LLMs. Arch: partial. Size: large. Mission: mixed. Scope: skills, agent runtime.
- `e9b0823db90f3229ca4a064ef0f1ae799e9bf6db` [fix] Remove nonexistent webhook secret command hint. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow src.
- `bedc71ebdcdc93a605f3bce8e724c78893d090ff` [fix] Cap retry-after delays. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate LLM stack, workspace/memory.
- `33a2dd2c78b25b3f333b9924ae7186bf637ac83f` [fix] Preserve polling after secret-blocked updates. Main: maybe. Severity: high. Effectiveness: targeted. Scope/blast: narrow WASM channels.
- `0be591028add18965ce142bf195f38f33fc11d64` [fix] Retry after missing session id errors. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow src.

### 2026-03-19

- `92869785474f4fe5fea9d95fedddb8f728ceaa19` [chore] Add coverage gates via codecov.yml (#1228). Need main: yes. Chore note: quality or supply-chain hygiene change worth mirroring if the equivalent gate is missing.

### 2026-03-18

- `2d0b195321531618fdb728f0db178edf9cf2745c` [feat] Upgrade MiniMax default model to M2.7. Arch: low. Size: medium. Mission: weak. Scope: .env.example, docs.

### 2026-03-19

- `07e6e30ee3e6dd1ecbdbf46a65e08e50d16e82fe` [fix] Add debug_assert invariant guards to critical code paths. Main: maybe. Severity: medium. Effectiveness: comprehensive. Scope/blast: moderate src, LLM stack.
- `f2cd1d37bc34f1017d617b1902a32fd1078ea23e` [docs] Add Japanese README. Need main: maybe. Docs note: moderate change across docs; only worth porting if the same surface is active in Axinite.

### 2026-03-18

- `20202700dbef968297e24976ed45edaae10ce135` [fix] Fix duplicate LLM responses for matched event routines. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate agent runtime, tests.
- `42ffefabe4003368e75e6470d48d40528b81d8ef` [fix] Remove -x from coverage pytest to prevent suite-blocking failures. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: narrow CI/release.
- `6831bb4d7b2bf7bf841c07de098ec023ddb26a5c` [fix] Full_job routine concurrency tracks linked job lifetime. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate agent runtime, tests.
- `59acab43f4666489d081436e2c57c4d2a95afeab` [merge] Merge pull request #1359 from nearai/staging-promote/428303af-23255149035. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `2033d7757928f16c14cc2f24ec3ebd15f30b64fa` [merge] Merge pull request #1376 from nearai/staging-promote/f2cd1d37-23262791325. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `a95a84ea79ff0016655244b30603f698662265d7` [merge] Merge pull request #1379 from nearai/staging-promote/6831bb4d-23264725970. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `14abd609179a66cc735f2342fa92cdc60bfc0bd9` [fix] Full_job routine runs stay running until linked job completion. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: very broad agent runtime, database.
- `ec04354c6b031ff45b10c88592813f9b01564a22` [fix] Address valid review comments from PR #1359. Main: maybe. Severity: medium. Effectiveness: follow-up. Scope/blast: moderate agent runtime, tool runtime.
- `4566181f40d1bdf7546d101758d22187f6ab7fb8` [feat] Unified settings page with subtabs. Arch: direct. Size: very large. Mission: mixed-strong. The unified settings UI aligns with Axinite's accessible web gateway goals, but the exact structure should still follow the fork's information architecture rather than land wholesale.
- `b7a1edf346e352590fa1c07d1807ac7c98c53a8c` [fix] Remove debug_assert guards that panic on valid error paths. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow src.
- `94e4d9d3ddfd08c93a11c5a10b2be82c7e0168f9` [merge] Merge pull request #1389 from nearai/main. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `44d16732a703c0c7f54f019b1d4d5806211c8515` [merge] Merge pull request #1390 from nearai/staging-promote/94e4d9d3-23273403042. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `8b15f8b259db9c269a418e2894f024d6671f8c57` [feat] Support auto split large message. Arch: direct. Size: small. Mission: strong. Scope: channels-src.
- `c8ee55ed194a0df8605c4242b8d27ea7fc2387e1` [feat] Add FaultInjector framework for StubLlm. Arch: partial. Size: small. Mission: mixed. Scope: src.
- `3dcccc1e64ea92fef2a44cf413b7cf974821da96` [feat] Wire stuck_threshold, store, and builder. Arch: partial. Size: large. Mission: mixed. Scope: agent runtime, src.
- `b9e5acf66e44fcb7e38c795cbdf96ea0ded553cf` [fix] Add missing `builder` field and update E2E extensions tab navigation. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow tests.

### 2026-03-19

- `07c6ca72e9e6512e687fba6c3acb79aeb5991702` [fix] Navigate telegram E2E tests to channels subtab. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: narrow tests.
- `0e3aa4f806119644fc6a342639a72c808997e793` [merge] Merge pull request #1409 from nearai/staging-promote/07c6ca72-23302016242. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `656d1f3e868220cb775c4eae7cf42c19a672d7dd` [merge] Merge pull request #1402 from nearai/staging-promote/b9e5acf6-23283208580. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `e582166781615464cbc38a93e6a90fdf4683b3fb` [merge] Merge pull request #1396 from nearai/staging-promote/3dcccc1e-23280048384. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.

### 2026-03-20

- `9c34fe90f40df52bb735677e8fc700c0587d229a` [chore] Enforce test requirement for state machine and resilience changes (#1230). Need main: no. Chore note: repository maintenance with no obvious fork-specific action.

### 2026-03-19

- `38dafb96b1c24ca68f945d5281af1c6b5f0bef6a` [chore] Bump telegram channel version to 0.2.5. Need main: no. Chore note: repository maintenance with no obvious fork-specific action.
- `e1d9827b21a1be97f707a2b172db29f85a117228` [merge] Merge pull request #1411 from nearai/staging-promote/38dafb96-23306226661. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `71f9012de37f663ce967cd1068ef7f381b287a56` [fix] Skip NEAR AI session check when backend is not nearai. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.
- `71f41dd12363497372864bc6eb3f7c334e05fd52` [fix] Parse flat token response from tenant_access_token API. Main: unlikely. Severity: medium. Effectiveness: targeted. Scope/blast: narrow channels-src.
- `e1774e9ec052f715785a6bb2321c394f433bff57` [merge] Merge pull request #1387 from nearai/staging-promote/ec04354c-23271447493. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `7dc3c6d0670e316145f38ea5d79eefd17e4c978c` [chore] Release v0.20.0. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `09e1c97a27bf58760e161fbefb76f3d2085faffc` [fix] Make "always" auto-approve work for credentialed HTTP requests. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: very broad agent runtime, src.
- `52ca9d6588f31fc9b6007c56ed7cd1995d5ad0df` [feat] Receive relay events via webhook callbacks. Arch: partial. Size: large. Mission: mixed. Scope: src, relay.
- `e4d3200d808737293337220403e21cf4a364e85b` [chore] Update WASM artifact SHA256 checksums [skip ci]. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `86ae12747bd872ea9ba8a210324b0004f4d96662` [feat] LRU embedding cache for workspace search. Arch: partial. Size: large. Mission: mixed. Scope: agent runtime, src.
- `65062f3cc069ebbd29f6d9be874ae5eff796e43a` [feat] Structured fallback deliverables for failed/stuck jobs. Arch: partial. Size: large. Mission: mixed. Scope: agent runtime, web gateway.
- `c4ab382522c86e7e19d55fee760b125fb1970518` [support] Make hosted OAuth and MCP auth generic. Need main: maybe. Supporting change touching parity docs, web gateway; evaluate only if dependent upstream fixes/features are adopted.
- `cac6f4013c3003c901aecc77fc6f32b8ef2718e0` [feat] Add owner-scoped permissions for full-job routines. Arch: partial. Size: very large. Mission: mixed. Scope: agent runtime, web gateway.
- `6b0f84bbe04edbfab2c8f0c5cda13c818e195dcc` [perf] Use Arc in embedding cache to avoid clones on miss path. Need main: maybe. Perf note: narrow change across workspace/memory; only worth porting if the same surface is active in Axinite.
- `8920322589143822cec05415be025435f25be6d4` [fix] Staging CI triage — consolidate retry parsing, fix flaky tests, add docs. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: broad docs, LLM stack.
- `8526cde1be0aa0e34c53aaf6833a80644c1aef97` [fix] Restore libSQL vector search with dynamic dimensions. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: broad config, database.
- `455f543ba50d610eb9e181fd41bf4c77615d3af6` [fix] Surface errors when sandbox unavailable for full_job routines. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: very broad agent runtime, database.
- `3a523347b0147ee07dc9fcd1d1e3107e8c3e1f14` [fix] F32→f64 precision artifact in temperature causes provider 400 errors. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: narrow LLM stack.
- `806d402876eae1e4c43a37fb51015d8e93af79fa` [feat] Chat onboarding and routine advisor. Arch: partial. Size: very large. Mission: mixed. Scope: .env.example, CLAUDE.md.
- `31c3b5b041f87909f74c6e5a1af6f64ce06f7d3f` [feat] Activate stuck_threshold for time-based stuck job detection. Arch: partial. Size: medium. Mission: mixed. Scope: agent runtime, src.
- `ef3d76974239f3113e390a3af9d0809c70af6492` [fix] Validate embedding base URLs to prevent SSRF. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: moderate config.
- `b952d229f941298af5748d421edca6513382f7f5` [fix] Prefer execution-local message routing metadata. Main: maybe. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate agent runtime, tool runtime.

### 2026-03-20

- `e82f4bd2e56f547079838f88b33ca731d1e921e6` [fix] Register sandbox jobs in ContextManager for query tool visibility. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: broad agent runtime, src.

### 2026-03-19

- `c17626160ce956a5e7c64a59b3e65c1801fee21f` [fix] Skip credential validation for Bedrock backend. Main: unlikely. Severity: medium. Effectiveness: targeted. Scope/blast: narrow src.

### 2026-03-20

- `1b97ef4feb07dfd24a878be9c3dd2fd32e1106d4` [fix] Resolve wasm broadcast merge conflicts with staging (#395). Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: moderate agent runtime, WASM channels.
- `cba1bc37997b2980e08ca9939747f9e2d7484102` [feat] Add light theme with dark/light/system toggle. Arch: direct. Size: medium. Mission: mixed-strong. A light theme and user-selectable theme mode fit the accessibility-first UI story, even though they are polish rather than core runtime capability.
- `3da9810e87b0c9e3ff8aaa3eb4dd21c5f5009d79` [feat] Add OpenAI Codex (ChatGPT subscription) as LLM provider. Arch: partial. Size: very large. Mission: mixed. Scope: .env.example, src.
- `d5e08b95f929a407b58ef3860e6cb646dffb57a4` [merge] Merge pull request #1439 from nearai/staging-promote/c4ab3825-23321164063. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `23263029f9489830db95699d25b9fd0c1051808f` [merge] Merge pull request #1428 from nearai/staging-promote/65062f3c-23317058602. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `e031d8246beaf89f7eef6bbdf81cd88738bd866f` [merge] Merge pull request #1425 from nearai/staging-promote/52ca9d65-23312673755. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `6fc8cc2f39ba688ae5aa620e6230dbf7ae215940` [merge] Merge pull request #1422 from nearai/staging-promote/71f41dd1-23309993684. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `ee6f5cd62abdc6086a9087f40bd51a53c79b7447` [feat] Use live owner tool scope for autonomous routines and jobs. Arch: partial. Size: very large. Mission: mixed. Scope: agent runtime, web gateway.
- `e077e1277d730601785738d9b8d4ba9aadaa8f29` [fix] Bump Feishu channel version for promotion. Main: unlikely. Severity: medium. Effectiveness: targeted. Scope/blast: narrow registry.
- `d1d74d665a38cf58bb0d3ae6056eccfed8f9d1cf` [merge] Merge pull request #1420 from nearai/staging-promote/71f9012d-23307625134. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `91a241a3c7fccb4d17d61c5d011d103968163b68` [chore] Release v0.21.0. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `d3b69e7be35217ebb6f6ce9fb3547402c862797e` [fix] Fix CI approval flows and stale fixtures. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: very broad docs, agent runtime.
- `d47b4b0346537497b47bad0488f3611f82c514ac` [chore] Update WASM artifact SHA256 checksums [skip ci]. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `9603fefd01645e4b0645512661581dd11402ef43` [fix] Remove redundant LLM config and API keys from bootstrap .env. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate LLM stack, src.
- `6d847c6009af6983305f2ee95943b4a38cfa35b2` [feat] Add public webhook trigger endpoint for routines. Arch: partial. Size: very large. Mission: mixed. Scope: agent runtime, web gateway.

### 2026-03-21

- `47ba4869908a04792b5b505284eac55cf00d9366` [docs] Expand AGENTS.md with coding agents guidance. Need main: maybe. Docs note: narrow change across AGENTS.md; only worth porting if the same surface is active in Axinite.

### 2026-03-20

- `c6d4abdb31b4f2e19b2149836d3ef1cb4a11ce35` [fix] Serialize env-mutating OAuth wildcard tests with ENV_MUTEX (#1280). Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow LLM stack.
- `a4f6cda5c9e0cd1d0f2d8809941e2927ffca2982` [fix] Add missing extension_manager field in trigger_manual EngineContext. Main: maybe. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow agent runtime.

### 2026-03-21

- `e6277a399f2eb0320fe0cca72f57d1ba24d2c161` [perf] Single-pass escape_xml_attr. Need main: maybe. Perf note: narrow change across crates; only worth porting if the same surface is active in Axinite.

### 2026-03-20

- `0d1a5c210b877f89bcb87e6f1d8584396d12f208` [fix] Patch rustls-webpki vulnerability (RUSTSEC-2026-0049). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate Cargo.lock, deny.toml.

### 2026-03-21

- `212d661e206033e4b77e4e484ed0e5a500e2fc89` [feat] Layered memory with sensitivity-based privacy redirect. Arch: conflicts. Size: large. Mission: mixed at best. Sensitivity-aware memory routing is adjacent to Axinite's privacy goals, but the implementation doubles down on in-process workspace layering instead of the planned memoryd sidecar boundary.

### 2026-03-20

- `9964d5dab8a1d59edb082edc64327519d6c20c4e` [feat] Include thumbnail URLs in search results. Arch: partial. Size: small. Mission: mixed. Scope: tools-src.
- `1d6f7d50850e30bc41ea85bb055dbd0af0655a29` [fix] Persist startup-loaded MCP clients in ExtensionManager. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate src, extensions/registry.

### 2026-03-21

- `62326090808b62267fad6ffd141db84f5e7dfebd` [feat] Add GitHub Copilot as LLM provider. Arch: low. Size: very large. Mission: weak. Scope: .env.example, parity docs.
- `8ad7d78a707bc12bf5fc3c3a8a07647962da6927` [fix] Parameter coercion and validation for oneOf/anyOf/allOf schemas. Main: maybe. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad tool runtime, src.
- `9d538136b5d86a1eb0a11ef469729b7304db24fb` [fix] Reject malformed ic2.* states in decode_hosted_oauth_state (#1441). Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow src.

### 2026-03-22

- `b97d82dbe6b32e859d6ec809353c9d52e0762149` [feat] Support text setup fields in web configure modal. Arch: partial. Size: large. Mission: mixed. Scope: web gateway, extensions/registry.

### 2026-03-21

- `189fc031e36f2a9a9ea82673e49680bcee6244ff` [merge] Merge branch 'staging' into fix/musl-installer-targets. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `07c338f55da7f1496a338810fddcdb1f8eccfe2c` [fix] Escape tool output XML content and remove misleading sanitized attr. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: very broad benches, crates.
- `0e5837b83a856d11b8d1896bc8fe38d6607000be` [merge] Merge pull request #1013 from rajulbhatnagar/fix/musl-installer-targets. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `89394ebd29e2b956f1e365c1cf502c36d207d12d` [feat] Add `ironclaw hooks list` subcommand. Arch: partial. Size: large. Mission: mixed. Scope: parity docs, src.
- `ccdea40e9d2d6c8e7beb1e5d454014dc51d2c8ff` [feat] Queue and merge messages during active turns. Arch: direct. Size: medium. Mission: strong. Better turn-queueing and merge behaviour improve the agent loop without widening the product surface.

### 2026-03-22

- `b58b421535e593b165393846a4c37d74283060ad` [feat] Add Low/Medium/High risk levels for graduated command approval (closes #172). Arch: direct. Size: large. Mission: strong. Graduated command approval tiers map well onto Axinite's explicit approval-boundary goals.
- `8638895879047fc900ee85720c0cafc6859c84d5` [feat] Full Gemini CLI OAuth integration with Cloud Code API. Arch: low. Size: large. Mission: weak. Scope: .env.example, parity docs.

### 2026-03-21

- `a09c0236421e02a70d76e7244b4fb5625c753fd6` [feat] Complete UX overhaul — design system, onboarding, web polish. Arch: direct. Size: very large. Mission: mixed-strong. Upstream's UX overhaul lands in the right area for Axinite's responsive accessible UI, but the fork should port selectively rather than absorb the full design system wholesale.

### 2026-03-22

- `1a62febe67cbf0fafffa3f6ee35fe751d39a5a4d` [perf] Avoid preview allocations for non-truncated strings (fix #894). Need main: maybe. Perf note: moderate change across agent runtime, sandbox/worker; only worth porting if the same surface is active in Axinite.
- `fbce9a5fe357601c2f0dd793fa150ff851407617` [refactor] Move transcription module into src/llm/. Need main: maybe. Refactor note: very broad change across agent runtime, config; only worth porting if the same surface is active in Axinite.
- `3aa36c8f55c61a9d9fcfabdbbae944ab0a46f130` [fix] Eliminate env mutex poison cascade. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: very broad CI/release, src.
- `969b559e2abca655731da98e85ca4b62313f77a7` [fix] Handle empty 202 notification acknowledgements. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow src.

### 2026-03-23

- `3e73dbe615683e8a4dec551793df5c85e8e631b9` [perf] Remove unconditional params clone in shared execution (fix #893). Need main: maybe. Perf note: broad change across agent runtime, src; only worth porting if the same surface is active in Axinite.
- `7034e910c4741ce0472c9e7b06d1b16ea53ad770` [fix] Generate Mistral-compatible 9-char alphanumeric tool call IDs. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: moderate agent runtime, LLM stack.
- `abba083147775f7d4b03a51a376cb1b7617cf7d1` [docs] Clarify webhook-only event subscription support. Need main: maybe. Docs note: narrow change across channels-src; only worth porting if the same surface is active in Axinite.

### 2026-03-22

- `4d7501a9684469998f2b518f6bd3da8bc95b266a` [fix] Fix owner-scoped message routing fallbacks. Main: maybe. Severity: medium-high. Effectiveness: targeted. Scope/blast: broad src, tool runtime.

### 2026-03-23

- `8f6999a0740a0222ecb52ddafb54084a97c75490` [docs] Add gitcgr code graph badge. Need main: maybe. Docs note: narrow change across docs; only worth porting if the same surface is active in Axinite.
- `d9358b0fa9a551dbad13a55aeaeaee923683394f` [feat] Multi-scope workspace reads. Arch: partial. Size: very large. Mission: mixed. Scope: src, web gateway.
- `acb590214a869747940ea31d47255c1ec0998070` [test] Google OAuth URL broken when initiated from Telegram channel. Need main: maybe. Test note: moderate change across CI/release, tests; only worth porting if the same surface is active in Axinite.
- `485d1568c46ff502e96f9dbdd83446800e43e7de` [feat] Add ironclaw models subcommands (list/status/set/set-provider). Arch: partial. Size: large. Mission: mixed. Scope: Cargo.lock, parity docs.
- `dea789cca9853ee814ae05c565de4e84684801b5` [feat] Default new lightweight routines to tools-enabled. Arch: partial. Size: medium. Mission: mixed. Scope: agent runtime, src.
- `bd6977e6a8766d0ca9ad336480f40916b5c93ab5` [merge] Merge pull request #1447 from nearai/staging-promote/89203225-23327092672. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `ddf64e8485b9756d9c13045aace9340e21937087` [merge] Merge pull request #1466 from nearai/staging-promote/3da9810e-23351687636. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `0194275792e9e9d94823a218602a4906ba1b2515` [merge] Merge pull request #1462 from nearai/staging-promote/cba1bc37-23334371795. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `bb57e36e6d5d45b042fb20fa1217ab4857de4c83` [merge] Merge pull request #1459 from nearai/staging-promote/c1762616-23332963145. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `74b2b4129ecaa5171f71f8a1eaf2bcd9e478f57c` [merge] Merge pull request #1456 from nearai/staging-promote/b952d229-23331469361. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `98418b3ef04d08d7b27dde1f478c4c8ea18cfc5b` [merge] Merge pull request #1452 from nearai/staging-promote/806d4028-23330265305. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `fa51b9f52dde0727f5dd65f134b93095832de959` [fix] Post-merge review sweep — 8 fixes across security, perf, and correctness. Main: yes. Severity: high. Effectiveness: comprehensive. Scope/blast: very broad agent runtime, WASM channels.
- `ae370d7e2b617d3d2eff5f9a25fada2a2dddaf25` [merge] Merge pull request #1467 from nearai/staging-promote/ee6f5cd6-23354122351. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.

### 2026-03-24

- `b441ebec02bdedf650abbcc89c6321b477247504` [feat] Multi-tenant auth with per-user workspace isolation. Arch: conflicts. Size: very large. Mission: no. Multi-tenant auth and per-user workspace isolation pull the runtime away from Axinite's single-user process model.

### 2026-03-23

- `3fdb18779699b68a7d429048a0b232e7afffff3c` [refactor] Auto-compact WASM tool schemas, add descriptions, improve credential prompts. Need main: maybe. Refactor note: very broad change across channels-src, WASM tools; only worth porting if the same surface is active in Axinite.
- `5847479fd851726e7e1e848b45bcf48a195f9aa9` [fix] Persist /model selection to .env, TOML, and DB. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: broad agent runtime, src.

### 2026-03-24

- `fb3548956bf6b1cc4fb31cb753b4fa24a7cfec68` [fix] Managed tunnels target wrong port and die from SIGPIPE. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate src.
- `01678be61d6a95ed3051772f6fe128b63c187b1e` [fix] Normalize status display across web and CLI. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: very broad web gateway, src.
- `d3d517fd677f3f1f32f7351df8b310229fb5fba9` [fix] Case-insensitive channel match and user_id filter for event triggers. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate agent runtime, tests.
- `5901451603d164a0e5814855161e5d5b05cc0cf0` [fix] Remove stale stream_token gate from channel-relay activation. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate extensions/registry, src.
- `424b470c5947206ce6f3848cbfefe77c785616a7` [merge] Merge pull request #1483 from nearai/staging-promote/d3b69e7b-23359661011. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `f3da30a4549947e715891732b966b56b73f56fa0` [perf] Optimize approval thread resolution (UUID parsing + lock contention). Need main: maybe. Perf note: narrow change across agent runtime; only worth porting if the same surface is active in Axinite.
- `dcb2d89e3a5ed19b30878557adfe505b66484483` [fix] Fix hosted OAuth refresh via proxy. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: very broad src, extensions/registry.
- `82822d7b2556a1cf29c6525d211cadd9b0a5917f` [fix] Restore owner-scoped gateway startup. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: very broad src, web gateway.
- `656151783cb9aa165d9dc99e82d7855ed3943b11` [feat] Show credential auth status in tool info. Arch: partial. Size: small. Mission: mixed. Scope: src.
- `706c3a1b4747d0335fd45013deddde3239be2f7f` [refactor] Extract AppEvent to crates/ironclaw_common. Need main: maybe. Refactor note: very broad change across Cargo.lock, Cargo.toml; only worth porting if the same surface is active in Axinite.

### 2026-03-25

- `6daa2f155f2683cf93669cac5844b6d85400b7a5` [fix] Ensure LLM calls always end with user message (closes #763). Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: very broad agent runtime, LLM stack.
- `67a025e2faf73c9f970129523c7bc18b5d3c3c9e` [fix] Unblock promotion PR #1451 cargo-deny. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate Cargo.lock, deny.toml.
- `41ed0a0f9814d754c17df80c14d263ae10e09b45` [feat] Thread per-tool reasoning through provider, session, and all surfaces. Arch: partial. Size: very large. Mission: mixed. Scope: crates, agent runtime.
- `0341fcc9405e3a9f22319891dc1d55d3a67edc06` [fix] Fix REPL single-message hang and cap CI test duration. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: very broad CI/release, agent runtime.
- `c949521d8d153ecb3af30877779f8c160278ca09` [fix] Fix MCP lifecycle trace user scope. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow tests.
- `ab0ad948f36c7cc88b1aecf2e92dd0ff94569a94` [fix] Normalize cron schedules on routine create. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate tool runtime, tests.
- `86d11430640da22d8f890bb9b2df867dda1e668e` [fix] Fix libsql prompt scope regressions. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: broad agent runtime, workspace/memory.
- `5a5ffe8d08364d75b110002a999b7e5a71548fd0` [merge] Merge pull request #1654 from nearai/staging-promote/86d11430-23565413131. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `c5dce279e23318c15263ff7edfeaf88603a17ef9` [merge] Merge pull request #1649 from nearai/staging-promote/ab0ad948-23563320113. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `189fa35e640e3eb43f49a5c05ce6debba1089eda` [merge] Merge pull request #1647 from nearai/staging-promote/c949521d-23562109203. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `c98ec3fb189684a54e15bec7ea30a5a132bea886` [merge] Merge pull request #1645 from nearai/staging-promote/0341fcc9-23558273569. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `b8b88ab84ea9c3c564d12d4071b2093e5d3ba408` [merge] Merge pull request #1642 from nearai/staging-promote/6daa2f15-23538193544. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `492d9d22c95eea793bb0142c0a2eb490bfa5b3d3` [merge] Merge pull request #1627 from nearai/staging-promote/82822d7b-23516534944. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `9fd5537a01f66282b7d4491bb2e514b0d922f467` [merge] Merge pull request #1624 from nearai/staging-promote/59014516-23505370929. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `0145672f365590d42d6dc55d7be011c652d444ad` [merge] Merge pull request #1620 from nearai/staging-promote/d3d517fd-23491969691. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `c737fb08557440a63ce0a7ec4d5b757fddf35371` [merge] Merge pull request #1616 from nearai/staging-promote/fb354895-23477842664. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `a23d87fc00948c6e77d1bd2bd7cce95c5cb8f66c` [merge] Merge pull request #1606 from nearai/staging-promote/fa51b9f5-23468747429. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `d4e18020e2b9b3aeb303798d6e4a178377938451` [merge] Merge pull request #1604 from nearai/staging-promote/dea789cc-23455694329. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `e15c50ea2d030fae36120310f7dc51c8d6f4b135` [merge] Merge pull request #1593 from nearai/staging-promote/485d1568-23439773006. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `ad20a5ab4f78f53ac808dfd8b85390fe8257b88f` [merge] Merge pull request #1583 from nearai/staging-promote/d9358b0f-23426138451. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `1f8d901cf6ec19e2f0aaf33a2542c10ee64e52c3` [merge] Merge pull request #1576 from nearai/staging-promote/abba0831-23415935143. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `2f47c611d4b4164dfe8f1b461701000cac31b8db` [merge] Merge pull request #1561 from nearai/staging-promote/fbce9a5f-23403885064. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `2f80b7b0b8f832e065915f08a187280a77c4a1d6` [merge] Merge pull request #1560 from nearai/staging-promote/1a62febe-23398066063. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `a19deb6812696159d51bde9a715e8c8e79cdf7ae` [merge] Merge pull request #1556 from nearai/staging-promote/86388958-23397163010. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `16aaea8d7430bc41a0322946d89439999c3c9cfa` [merge] Merge pull request #1555 from nearai/staging-promote/b58b4215-23396456254. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `cb01800f73d220e93852789ec0315b26b24b0edd` [merge] Merge pull request #1553 from nearai/staging-promote/89394ebd-23395764012. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `f9dfb7480004893c1e88b9b54f18a0a11f144e3d` [merge] Merge pull request #1552 from nearai/staging-promote/b97d82db-23390775365. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `3d43917cd0244d58acdb9cb24a5beadac80fddc7` [merge] Merge pull request #1551 from nearai/staging-promote/9d538136-23389762470. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `5d714be354a1a69b11d7ce7a9cd249266f376d01` [merge] Merge pull request #1548 from nearai/staging-promote/8ad7d78a-23387609319. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `c0f33c37f7d02512cb52199248efd3c4f813ae37` [merge] Merge pull request #1522 from nearai/staging-promote/62326090-23374571867. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `2b4e881a721597e95586d456f5a517653cfbdb0e` [merge] Merge pull request #1517 from nearai/staging-promote/9964d5da-23372765633. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `4c5d961102f12b9869ad90890a05434047f28f15` [merge] Merge pull request #1515 from nearai/staging-promote/0d1a5c21-23372030005. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `8d632872fde024ad3d3e009095c67e6a597d18ed` [merge] Merge pull request #1514 from nearai/staging-promote/e6277a39-23371263100. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `ea24d79ace0172faafa5db743c3181bdfca8030a` [merge] Merge pull request #1508 from nearai/staging-promote/6d847c60-23366109539. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `b400c2a711b5b8897d163e1566fffa642ed3140c` [merge] Merge pull request #1499 from nearai/staging-promote/9603fefd-23364438978. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `ef37d705a16a3ef91bac753ea0c344c998d55a19` [merge] Merge pull request #1655 from nearai/codex/fix-staging-promotion-1451-version-bumps. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `bb24952622d40a33bcfd40884e4e884b6bf443b3` [merge] Merge branch 'main' into staging-promote/455f543b-23329172268. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `cdc625566ff6a18e3adb110f0bed44b0aa17e069` [merge] Merge pull request #1451 from nearai/staging-promote/455f543b-23329172268. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `0b4e7c761b3e12853d743ab5d454255334e2c1c6` [chore] Release v0.22.0. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.
- `4c043bf05767d7e1ab74552eb010182ec44b3222` [feat] Complete multi-tenant isolation — phases 2–4. Arch: conflicts. Size: very large. Mission: no. Axinite's architecture documents still describe a single-user local-first deployment; upstream multi-tenant isolation is a deliberate product-direction divergence, not a feature carry.
- `f02345fd1f9140573d341cf9b4028d55eb021a1d` [fix] Allow publishing ironclaw_common. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: narrow crates.
- `ab67f028860094dc8086f4e9866ed0e8ff44b3cd` [fix] Publish ironclaw_safety 0.2.0. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: very broad Cargo.lock, Cargo.toml.
- `6b8a38e147885aae6f8cfeb0627a79bec6eebb7c` [chore] Update WASM artifact SHA256 checksums [skip ci]. Need main: no. Chore note: upstream release, promotion, documentation, or artefact-maintenance housekeeping.

### 2026-03-26

- `b3fbef5287c84d0388fcdab1713c69d5ef62104a` [fix] Filter XML tool-call recovery by context. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow LLM stack.
- `ed4d92932ac5d2d9123a8448aac4627bb8bb2d7c` [fix] Discard truncated tool calls when finish_reason == Length (#1631). Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: broad agent runtime, LLM stack.
- `9c63d189b708d4cf6cb208ff43cc26633dd3c114` [merge] Merge pull request #1612 from nearai/main. Need main: no. Merge note: integration or promotion merge with no standalone audit action beyond whatever child commits already require.
- `adf4e25c8fdebeacb4bb99752861b61f556dc8db` [fix] Channel-relay auth dead-end, observability, and URL override. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: broad relay, web gateway.
- `1d5777824c617450ac2ce685d15b52c99ef69db3` [fix] Handle 202 Accepted and wire session manager for Streamable HTTP. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: moderate src.
- `dd0a0e10abebcd7c161c6e86fb89b8bd06e38592` [fix] Recover delete name after failed update fallback. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: broad src, tool runtime.
- `5b95d222186f9ee8f89edf69480000ca42f0d7d0` [feat] Support direct hosted OAuth callbacks with proxy auth token. Arch: partial. Size: medium. Mission: mixed. Scope: web gateway, src.

### 2026-03-27

- `45cd6682d3435b4652c062f768e1d5314237dafd` [fix] Downgrade excessive debug logging in hot path (closes #1686). Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: broad relay, web gateway.
- `9c5ba43ccd493c7fad40575ac5210b92648d9545` [feat] Add OpenAI Responses API endpoints. Arch: partial. Size: very large. Mission: strong but at the wrong seam. Axinite wants OpenAI Responses over WebSocket as a provider/runtime path; upstream ships gateway HTTP endpoints. Useful reference material, not a straight cherry-pick.
- `7234700c78d985ddc872721bc2a7130eeaa0b8c3` [fix] Prevent UTF-8 panic in line_bounds() (fixes #1669). Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: moderate web gateway, LLM stack.
- `30db07c58e02fa9bb3a6757823e9aa64322aee6f` [fix] Require Feishu webhook authentication. Main: unlikely. Severity: medium-high. Effectiveness: targeted. Scope/blast: broad channels-src, WASM channels.
- `2f4eb08613cefff1af8b7b1a475fda00c84dd855` [fix] Sanitize tool error results before llm injection. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: broad agent runtime, web gateway.

### 2026-03-28

- `8f8cb7f7b1767d28f62ec74a14bf8eb74dd1097a` [feat] DB-backed user management, admin secrets provisioning, and multi-tenant isolation. Arch: conflicts. Size: very large. Mission: no. DB-backed user management and admin provisioning continue the multi-tenant product direction that Axinite is explicitly avoiding.
- `f49f3683555de0f65a53918d9feb37ca4d0eeecb` [fix] Clean up extension credentials on uninstall. Main: maybe. Severity: medium. Effectiveness: targeted. Scope/blast: moderate extensions/registry, tests.
- `27e8d6f8dd106c462c7d616ae8c13f78a6d8b423` [fix] Use typed WASM schema as advertised schema when available. Main: yes. Severity: high. Effectiveness: targeted. Scope/blast: narrow WASM schema publication path. Canonical typed schemas are core to Axinite's tool-definition roadmap.
- `9ba10eac35debd6b77a6b4acbaf1b65a96e52a86` [fix] Add tracing warn for naive timestamp fallback and improve parse_timestamp tests. Main: maybe. Severity: low. Effectiveness: targeted. Scope/blast: narrow database.
- `0b33ca99262925558760fcfa2930bee60fe65997` [fix] Tighten legacy state validation and fallback handling. Main: yes. Severity: medium-high. Effectiveness: targeted. Scope/blast: narrow src.
- `9bb19a98f767f2e4db0d4f3dda0e495a356a726a` [fix] Redact database error details from API responses. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: narrow web API surface. Redacting database errors belongs in any user-facing Axinite gateway.
- `9ce3a9fc53707aa80cd8c8a6276e63adc4f61280` [feat] Implement on_broadcast via DM channel creation. Arch: low. Size: small. Mission: weak. Scope: channels-src.

### 2026-03-29

- `de5a1c7b0d0588e1898458870a0796e6dd8a361e` [fix] Replace script -qfc with pty-process for injection-safe PTY. Main: yes. Severity: high. Effectiveness: strong. Scope/blast: medium worker/PTY boundary. Replacing shell-mediated PTY spawning with an injection-safe process wrapper is directly relevant to Axinite's constrained codemode ambitions.
- `fd41bdf4bed3c9b43cf12788b717ac4c0fa8b5b5` [fix] Treat empty LLM response after text output as completion. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: moderate LLM stack, sandbox/worker.

### 2026-03-28

- `8a320ae9db4f7fdada609a30528bee6116cbe71c` [fix] Complete full_job execution reliability overhaul. Main: maybe. Severity: medium. Effectiveness: comprehensive. Scope/blast: very broad Cargo.toml, agent runtime.

### 2026-03-29

- `a8e83210ff01e7317f7af96bb3dc5b705ab7a63b` [feat] Add gateway channel flow in wasm. Arch: low. Size: large. Mission: weak. Scope: Discord gateway-in-WASM integration rather than a core Axinite target surface.
- `e0e530e646c4cdb05a2ea0cac8534d6e5fbb6afc` [docs] Tighten contribution and PR guidance. Need main: maybe. Docs note: moderate change across CI/release, CONTRIBUTING.md; only worth porting if the same surface is active in Axinite.
- `6f6a5f1dbdea2b291c9922696487180d09953bef` [feat] Recursive bundle directory scanning for skill discovery. Arch: direct. Size: medium. Mission: strong. Recursive discovery supports Axinite's multi-file skill-bundle direction, even though the fork still needs the stricter `.skill` packaging and read-surface work from RFC 0003.
- `d97c0145cf8b67b1b2be4ddbb49f798d5fa90d33` [support] Clarify message tool vs channel setup guidance. Need main: maybe. Supporting note: documentation and UX clarification worth mirroring only if the fork keeps the same setup split and terminology.
- `fcab4f0adaa94feb533ace75afe53838aecb8390` [fix] Pin staging ci jobs to a single tested sha. Main: no. Severity: low. Effectiveness: targeted. Scope/blast: moderate CI/release.
- `86389dab23ef4c49c56caf8eb0e9da451d916798` [fix] Preserve thought signatures on all tool calls. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: narrow LLM stack.
- `64fe9ba6077e3478e2684daf97d43556aff6996d` [fix] Prevent UTF-8 panics in byte-index string truncation. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: narrow string-handling helpers with potentially broad runtime reach. UTF-8 panic fixes are low-cost defensive carry items.
- `70214c4ae1f85436d00c0652042d44557ad8559f` [fix] Strip tool blocks from messages when toolConfig is absent. Main: unlikely. Severity: medium. Effectiveness: targeted. Scope/blast: narrow LLM stack.

### 2026-03-30

- `de384b0cc751c85b8faf5af18bc95fcd3633370f` [feat] Support custom LLM provider configuration via web UI. Arch: partial. Size: very large. Mission: weak-mixed. Configuring arbitrary providers through the web UI broadens the surface area beyond the fork's narrow focus; useful ideas exist in the secret-handling work, but the feature as a whole is not a clean fit.

### 2026-03-29

- `4f277c91be53366caaad00cfbf4245693dcd2ac7` [fix] Handle empty tool completions in autonomous jobs. Main: yes. Severity: medium. Effectiveness: targeted. Scope/blast: very broad agent runtime, LLM stack.

### 2026-03-30

- `368d2f523868cc06a3a84fc1255dd7411f680da6` [feat] OIDC JWT authentication for reverse-proxy deployments. Arch: conflicts. Size: medium. Mission: no. Reverse-proxy OIDC JWT auth assumes a hosted multi-user gateway posture that the single-user local-first fork does not target.

<!-- markdownlint-enable MD013 MD024 -->
