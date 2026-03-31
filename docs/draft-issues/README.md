# Draft issue set for upstream audit follow-up

Total draft issues: 132.

- Resolve deferred review items from PRs #883, #848, #788
  (`001-resolve-deferred-review-items-from-prs-883-848-788.md`)
- Replace regex HTML sanitizer with DOMPurify to prevent XSS
  (`012-replace-regex-html-sanitizer-with-dompurify-to-prevent-xss.md`)
- Resolve DNS once and reuse for SSRF validation to prevent rebinding
  (`014-resolve-dns-once-and-reuse-for-ssrf-validation-to-prevent-rebinding.md`)
- Load WASM tool description and schema from capabilities.json
  (`015-load-wasm-tool-description-and-schema-from-capabilities-json.md`)
- Drain residual events and filter key kind in onboard prompts (#937)
  (`017-drain-residual-events-and-filter-key-kind-in-onboard-prompts-937.md`)
- Stdio/unix transports skip initialize handshake (#890)
  (`018-stdio-unix-transports-skip-initialize-handshake-890.md`)
- Block thread_id-based context pollution across users
  (`019-block-thread-id-based-context-pollution-across-users.md`)
- Header safety validation and Authorization conflict bug from #704
  (`020-header-safety-validation-and-authorization-conflict-bug-from-704.md`)
- Drain tunnel pipes to prevent zombie process
  (`021-drain-tunnel-pipes-to-prevent-zombie-process.md`)
- Validate channel credentials during setup
  (`022-validate-channel-credentials-during-setup.md`)
- Fix systemctl unit (`027-fix-systemctl-unit.md`)
- Add Content-Security-Policy header to web gateway
  (`028-add-content-security-policy-header-to-web-gateway.md`)
- Require explicit SANDBOX_ALLOW_FULL_ACCESS to enable FullAccess policy
  (`029-require-explicit-sandbox-allow-full-access-to-enable-fullaccess-policy.md`)
- Make unsafe env::set_var calls safe with explicit invariants
  (`030-make-unsafe-env-set-var-calls-safe-with-explicit-invariants.md`)
- Migrate webhook auth to HMAC-SHA256 signature header
  (`031-migrate-webhook-auth-to-hmac-sha256-signature-header.md`)
- Attach session manager for non-OAuth HTTP clients (#793)
  (`032-attach-session-manager-for-non-oauth-http-clients-793.md`)
- Preserve model selection on provider re-run (#679)
  (`033-preserve-model-selection-on-provider-re-run-679.md`)
- Use versioned artifact URLs and checksums for all WASM manifests
  (`041-use-versioned-artifact-urls-and-checksums-for-all-wasm-manifests.md`)
- Release lock guards before awaiting channel send (#869)
  (`042-release-lock-guards-before-awaiting-channel-send-869.md`)
- Harden production container and bootstrap security
  (`043-harden-production-container-and-bootstrap-security.md`)
- Fix UTF-8 unsafe truncation in WASM emit_message
  (`044-fix-utf-8-unsafe-truncation-in-wasm-emit-message.md`)
- Open MCP OAuth in same browser as gateway
  (`045-open-mcp-oauth-in-same-browser-as-gateway.md`)
- Include OAuth state parameter in authorization URLs
  (`046-include-oauth-state-parameter-in-authorization-urls.md`)
- Remove all inline event handlers for CSP script-src compliance
  (`047-remove-all-inline-event-handlers-for-csp-script-src-compliance.md`)
- Reject absolute filesystem paths with corrective routing
  (`049-reject-absolute-filesystem-paths-with-corrective-routing.md`)
- Recompute cron next_fire_at when re-enabling routines
  (`062-recompute-cron-next-fire-at-when-re-enabling-routines.md`)
- Run cron checks immediately on ticker startup
  (`063-run-cron-checks-immediately-on-ticker-startup.md`)
- Make approval requests appear without page reload (#996)
  (`064-make-approval-requests-appear-without-page-reload-996.md`)
- Relax approval requirements for low-risk tools
  (`065-relax-approval-requirements-for-low-risk-tools.md`)
- Set CLI_ENABLED=false in macOS launchd plist
  (`066-set-cli-enabled-false-in-macos-launchd-plist.md`)
- Fail closed when webhook secret is missing at runtime
  (`067-fail-closed-when-webhook-secret-is-missing-at-runtime.md`)
- Add tool_info schema discovery for WASM tools
  (`072-add-tool-info-schema-discovery-for-wasm-tools.md`)
- Fix lifecycle bugs + comprehensive E2E tests
  (`073-fix-lifecycle-bugs-comprehensive-e2e-tests.md`)
- Bump telegram channel version for capabilities change
  (`077-bump-telegram-channel-version-for-capabilities-change.md`)
- 5 critical/high-priority bugs (auth bypass, relay failures, unbounded
  recursion, context growth)
  (`086-5-critical-high-priority-bugs-auth-bypass-relay-failures-unbounded-recursion-con.md`)
- Treat empty timezone string as absent
  (`088-treat-empty-timezone-string-as-absent.md`)
- Replace .expect() with match in webhook handler
  (`089-replace-expect-with-match-in-webhook-handler.md`)
- Address 14 audit findings across MCP module
  (`090-address-14-audit-findings-across-mcp-module.md`)
- HTTP webhook secret transmitted in request body rather than via header, docs
  inconsistency and security concern
  (`098-http-webhook-secret-transmitted-in-request-body-rather-than-via-header-docs-inco.md`)
- Google Sheets returns 403 PERMISSION_DENIED after completing OAuth
  (`099-google-sheets-returns-403-permission-denied-after-completing-oauth.md`)
- Avoid lock-held awaits in server lifecycle paths
  (`102-avoid-lock-held-awaits-in-server-lifecycle-paths.md`)
- Non-transactional multi-step context updates between metadata/to…
  (`103-non-transactional-multi-step-context-updates-between-metadata-to.md`)
- Use live owner binding during wasm hot activation
  (`105-use-live-owner-binding-during-wasm-hot-activation.md`)
- Add stop_sequences parity for tool completions
  (`106-add-stop-sequences-parity-for-tool-completions.md`)
- N+1 query pattern in event trigger loop (routine_engine)
  (`107-n-1-query-pattern-in-event-trigger-loop-routine-engine.md`)
- Update yanked uds_windows 1.2.0 -> 1.2.1
  (`109-update-yanked-uds-windows-1-2-0-1-2-1.md`)
- Fix schema-guided tool parameter coercion
  (`112-fix-schema-guided-tool-parameter-coercion.md`)
- Eliminate panic paths in production code
  (`113-eliminate-panic-paths-in-production-code.md`)
- Handle 400 auth errors, clear auth mode after OAuth, trim tokens
  (`118-handle-400-auth-errors-clear-auth-mode-after-oauth-trim-tokens.md`)
- Prevent Safari IME composition Enter from sending message
  (`119-prevent-safari-ime-composition-enter-from-sending-message.md`)
- Preserve AuthError type in oauth_http_client cache
  (`120-preserve-autherror-type-in-oauth-http-client-cache.md`)
- Treat empty url param as absent when installing skills
  (`121-treat-empty-url-param-as-absent-when-installing-skills.md`)
- Normalize chat copy to plain text (`122-normalize-chat-copy-to-plain-text.md`)
- Unify ChannelsConfig resolution to env > settings > default
  (`123-unify-channelsconfig-resolution-to-env-settings-default.md`)
- Fix subagent monitor events being treated as user input
  (`124-fix-subagent-monitor-events-being-treated-as-user-input.md`)
- Avoid false success and block chat during pending auth
  (`125-avoid-false-success-and-block-chat-during-pending-auth.md`)
- Default webhook server to loopback when tunnel is configured
  (`126-default-webhook-server-to-loopback-when-tunnel-is-configured.md`)
- Fix conflict (`127-fix-conflict.md`)
- Prevent metadata spoofing of internal job monitor flag
  (`129-prevent-metadata-spoofing-of-internal-job-monitor-flag.md`)
- Telegram bot token validation fails intermittently (HTTP 404)
  (`132-telegram-bot-token-validation-fails-intermittently-http-404.md`)
- Prevent orphaned tool_results and fix parallel merging
  (`136-prevent-orphaned-tool-results-and-fix-parallel-merging.md`)
- Persist refreshed Anthropic OAuth token after Keychain re-read
  (`137-persist-refreshed-anthropic-oauth-token-after-keychain-re-read.md`)
- Make completed->completed transition idempotent to prevent race errors
  (`138-make-completed-completed-transition-idempotent-to-prevent-race-errors.md`)
- Web/CLI routine mutations do not refresh live event trigger cache
  (`157-web-cli-routine-mutations-do-not-refresh-live-event-trigger-cache.md`)
- Resolve merge conflict fallout and missing config fields
  (`167-resolve-merge-conflict-fallout-and-missing-config-fields.md`)
- Bump channel registry versions for promotion
  (`172-bump-channel-registry-versions-for-promotion.md`)
- Misleading UI message (`174-misleading-ui-message.md`)
- Jobs limit (`177-jobs-limit.md`)
- Fix Telegram auto-verify flow and routing
  (`179-fix-telegram-auto-verify-flow-and-routing.md`)
- Rate limiter returns retry after None instead of a duration
  (`183-rate-limiter-returns-retry-after-none-instead-of-a-duration.md`)
- Mark ironclaw_safety unpublished in release-plz
  (`185-mark-ironclaw-safety-unpublished-in-release-plz.md`)
- Remove nonexistent webhook secret command hint
  (`193-remove-nonexistent-webhook-secret-command-hint.md`)
- Cap retry-after delays (`194-cap-retry-after-delays.md`)
- Preserve polling after secret-blocked updates
  (`195-preserve-polling-after-secret-blocked-updates.md`)
- Retry after missing session id errors
  (`196-retry-after-missing-session-id-errors.md`)
- Add debug_assert invariant guards to critical code paths
  (`199-add-debug-assert-invariant-guards-to-critical-code-paths.md`)
- Fix duplicate LLM responses for matched event routines
  (`201-fix-duplicate-llm-responses-for-matched-event-routines.md`)
- Full_job routine concurrency tracks linked job lifetime
  (`203-full-job-routine-concurrency-tracks-linked-job-lifetime.md`)
- Full_job routine runs stay running until linked job completion
  (`207-full-job-routine-runs-stay-running-until-linked-job-completion.md`)
- Address valid review comments from PR #1359
  (`208-address-valid-review-comments-from-pr-1359.md`)
- Remove debug_assert guards that panic on valid error paths
  (`210-remove-debug-assert-guards-that-panic-on-valid-error-paths.md`)
- Add missing `builder` field and update E2E extensions tab navigation
  (`216-add-missing-builder-field-and-update-e2e-extensions-tab-navigation.md`)
- Skip NEAR AI session check when backend is not nearai
  (`224-skip-near-ai-session-check-when-backend-is-not-nearai.md`)
- Make "always" auto-approve work for credentialed HTTP requests
  (`228-make-always-auto-approve-work-for-credentialed-http-requests.md`)
- Restore libSQL vector search with dynamic dimensions
  (`237-restore-libsql-vector-search-with-dynamic-dimensions.md`)
- Surface errors when sandbox unavailable for full_job routines
  (`238-surface-errors-when-sandbox-unavailable-for-full-job-routines.md`)
- Validate embedding base URLs to prevent SSRF
  (`242-validate-embedding-base-urls-to-prevent-ssrf.md`)
- Prefer execution-local message routing metadata
  (`243-prefer-execution-local-message-routing-metadata.md`)
- Register sandbox jobs in ContextManager for query tool visibility
  (`244-register-sandbox-jobs-in-contextmanager-for-query-tool-visibility.md`)
- Resolve wasm broadcast merge conflicts with staging (#395)
  (`246-resolve-wasm-broadcast-merge-conflicts-with-staging-395.md`)
- Remove redundant LLM config and API keys from bootstrap .env
  (`259-remove-redundant-llm-config-and-api-keys-from-bootstrap-env.md`)
- Serialize env-mutating OAuth wildcard tests with ENV_MUTEX (#1280)
  (`262-serialize-env-mutating-oauth-wildcard-tests-with-env-mutex-1280.md`)
- Add missing extension_manager field in trigger_manual EngineContext
  (`263-add-missing-extension-manager-field-in-trigger-manual-enginecontext.md`)
- Patch rustls-webpki vulnerability (RUSTSEC-2026-0049)
  (`265-patch-rustls-webpki-vulnerability-rustsec-2026-0049.md`)
- Persist startup-loaded MCP clients in ExtensionManager
  (`268-persist-startup-loaded-mcp-clients-in-extensionmanager.md`)
- Parameter coercion and validation for oneOf/anyOf/allOf schemas
  (`270-parameter-coercion-and-validation-for-oneof-anyof-allof-schemas.md`)
- Reject malformed ic2.* states in decode_hosted_oauth_state (#1441)
  (`271-reject-malformed-ic2-states-in-decode-hosted-oauth-state-1441.md`)
- Escape tool output XML content and remove misleading sanitized attr
  (`274-escape-tool-output-xml-content-and-remove-misleading-sanitized-attr.md`)
- Handle empty 202 notification acknowledgements
  (`284-handle-empty-202-notification-acknowledgements.md`)
- Generate Mistral-compatible 9-char alphanumeric tool call IDs
  (`286-generate-mistral-compatible-9-char-alphanumeric-tool-call-ids.md`)
- Fix owner-scoped message routing fallbacks
  (`288-fix-owner-scoped-message-routing-fallbacks.md`)
- Post-merge review sweep — 8 fixes across security, perf, and correctness
  (`300-post-merge-review-sweep-8-fixes-across-security-perf-and-correctness.md`)
- Persist /model selection to .env, TOML, and DB
  (`304-persist-model-selection-to-env-toml-and-db.md`)
- Managed tunnels target wrong port and die from SIGPIPE
  (`305-managed-tunnels-target-wrong-port-and-die-from-sigpipe.md`)
- Case-insensitive channel match and user_id filter for event triggers
  (`307-case-insensitive-channel-match-and-user-id-filter-for-event-triggers.md`)
- Remove stale stream_token gate from channel-relay activation
  (`308-remove-stale-stream-token-gate-from-channel-relay-activation.md`)
- Fix hosted OAuth refresh via proxy
  (`311-fix-hosted-oauth-refresh-via-proxy.md`)
- Restore owner-scoped gateway startup
  (`312-restore-owner-scoped-gateway-startup.md`)
- Ensure LLM calls always end with user message (closes #763)
  (`315-ensure-llm-calls-always-end-with-user-message-closes-763.md`)
- Unblock promotion PR #1451 cargo-deny
  (`316-unblock-promotion-pr-1451-cargo-deny.md`)
- Fix MCP lifecycle trace user scope
  (`319-fix-mcp-lifecycle-trace-user-scope.md`)
- Normalize cron schedules on routine create
  (`320-normalize-cron-schedules-on-routine-create.md`)
- Fix libsql prompt scope regressions
  (`321-fix-libsql-prompt-scope-regressions.md`)
- Allow publishing ironclaw_common (`355-allow-publishing-ironclaw-common.md`)
- Publish ironclaw_safety 0.2.0 (`356-publish-ironclaw-safety-0-2-0.md`)
- Filter XML tool-call recovery by context
  (`358-filter-xml-tool-call-recovery-by-context.md`)
- Discard truncated tool calls when finish_reason == Length (#1631)
  (`359-discard-truncated-tool-calls-when-finish-reason-length-1631.md`)
- Channel-relay auth dead-end, observability, and URL override
  (`361-channel-relay-auth-dead-end-observability-and-url-override.md`)
- Handle 202 Accepted and wire session manager for Streamable HTTP
  (`362-handle-202-accepted-and-wire-session-manager-for-streamable-http.md`)
- Recover delete name after failed update fallback
  (`363-recover-delete-name-after-failed-update-fallback.md`)
- Prevent UTF-8 panic in line_bounds() (fixes #1669)
  (`367-prevent-utf-8-panic-in-line-bounds-fixes-1669.md`)
- Sanitize tool error results before llm injection
  (`369-sanitize-tool-error-results-before-llm-injection.md`)
- Clean up extension credentials on uninstall
  (`371-clean-up-extension-credentials-on-uninstall.md`)
- Use typed WASM schema as advertised schema when available
  (`372-use-typed-wasm-schema-as-advertised-schema-when-available.md`)
- Tighten legacy state validation and fallback handling
  (`374-tighten-legacy-state-validation-and-fallback-handling.md`)
- Redact database error details from API responses
  (`375-redact-database-error-details-from-api-responses.md`)
- Replace script -qfc with pty-process for injection-safe PTY
  (`377-replace-script-qfc-with-pty-process-for-injection-safe-pty.md`)
- Treat empty LLM response after text output as completion
  (`378-treat-empty-llm-response-after-text-output-as-completion.md`)
- Complete full_job execution reliability overhaul
  (`379-complete-full-job-execution-reliability-overhaul.md`)
- Preserve thought signatures on all tool calls
  (`385-preserve-thought-signatures-on-all-tool-calls.md`)
- Prevent UTF-8 panics in byte-index string truncation
  (`386-prevent-utf-8-panics-in-byte-index-string-truncation.md`)
- Handle empty tool completions in autonomous jobs
  (`389-handle-empty-tool-completions-in-autonomous-jobs.md`)
