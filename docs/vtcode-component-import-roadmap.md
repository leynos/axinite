# VTCode component import roadmap

## Status and ownership

**Proposed, 2026-09-06.** This companion roadmap implements the
[component import design](vtcode-component-import-design.md). It follows the
[streaming design PR #360](https://github.com/leynos/axinite/pull/360) and refers
to its [3.2 task breakdown](responses-streaming-roadmap.md) where necessary.

`VT.<phase>.<task>` identifiers are local to this companion roadmap. They do not
renumber the main roadmap or replace its Responses, operator CLI, or memory
workstreams. Each task below is Axinite-owned unless explicitly identified as
an upstream dependency. No checkbox records shipped implementation.

## 1. Establish a bounded adoption baseline

Objective: identify the smallest components that solve real Axinite needs.

Learning opportunity: test whether extraction reduces maintenance cost rather
than simply moving it across repository boundaries.

- [ ] VT.1.1. Record source revisions, supported feature sets, licence notices,
  dependency closures, and cold/warm build baselines for each candidate.
  - See design sections 1 and 2.
  - Success: the record distinguishes main, baseline, and capability trains;
    no imported feature relies on an assumed merge or a moving Git branch.
- [ ] VT.1.2. Define Axinite adapter contracts and reusable semantic fixtures.
  Requires VT.1.1 and streaming task 3.2.1.1 for stream-specific fixtures.
  - See design sections 3, 4, and 6.
  - Success: terminal, codec, and provider cases have separate dependency and
    behavioural gates; a buffered terminal pilot has no transport dependency.
- [ ] VT.1.3. Add minimal external-consumer probes and dependency checks.
  Requires VT.1.1; coordinate VTCode #88 from the start of each extraction.
  - See design sections 5, 7, and 8.
  - Success: probes fail on forbidden inward dependencies and use distributable
    packages outside the VTCode workspace, not workspace feature unification.

## 2. Import presentation without importing execution

Objective: offer a richer terminal while retaining the existing agent runtime.

Learning opportunity: validate that richer controls improve operator workflows
without introducing alternative command, approval, or lifecycle semantics.

- [ ] VT.2.1. Select a bounded UI dependency after VTCode #79 and #80 expose
  host-owned configuration and supervised sessions. Requires VT.1.1-VT.1.3.
  - See design sections 2, 3, and 5.
  - Success: a no-workspace consumer launches the real UI with custom commands;
    the closure excludes agent, provider, credential, and config-discovery code.
- [ ] VT.2.2. Implement `AxiniteTuiChannel` through `NativeChannel` and startup
  configuration, initially with buffered output. Requires VT.2.1.
  - See design section 3; coordinate the command-registry work in Axinite #59.
  - Success: submit, interrupt, exit, one-shot, and non-TTY semantics match the
    existing channel; unsupported steering, PTY, and editor controls stay off.
- [ ] VT.2.3. Project approvals, authentication challenges, notifications, and
  canonical transcript state with scoped identities. Requires VT.2.2.
  - See design sections 4 and 5.
  - Success: stale/cross-thread approvals fail; secret input never enters
    history; a notification cannot finalize an active response.
- [ ] VT.2.4. Connect safe stream previews to the UI view model. Requires
  VT.2.3 and streaming tasks 3.2.6.1-3.2.6.2.
  - See design section 4 and the streaming design's sink-release policy.
  - Success: append/snapshot/final reconciliation uses identity and revision;
    interrupted output remains explicit; no raw provider event reaches the UI.
- [ ] VT.2.5. Prove presentation parity, resource bounds, and accessible use.
  Requires VT.2.3 for buffered parity and VT.2.4 for streaming parity.
  - See design sections 5 and 8; require the UI lane of VTCode #88.
  - Success: scripted plain/rich runs yield identical canonical outcomes;
    queue saturation, terminal failure, resizing, Unicode, and shutdown pass;
    manual accessibility evidence precedes any default change.
- [ ] VT.2.6. Release an opt-in rich terminal with a plain fallback and upgrade
  record. Requires VT.2.5 for the enabled delivery modes.
  - See design section 8.
  - Success: default remains plain until a separate promotion decision;
    fallback never resubmits work or disables other channels.

## 3. Evaluate and import pure Responses support

Objective: remove duplicated protocol work without weakening Axinite contracts.

Learning opportunity: determine whether the extracted codec preserves all native
item semantics and offers a better maintenance path than the native decoder.

- [ ] VT.3.1. Compare VTCode #85's extracted codec against Axinite's native
  decoder using one sanitized corpus. Requires VT.1.2, VT.1.3, and streaming
  tasks 3.2.3.1-3.2.3.2. Independent of the terminal lane.
  - See design section 6.
  - Success: segmentation, interleaving, malformed arguments, omitted terminal
    calls, opaque items, and absent usage have explicit differential results.
- [ ] VT.3.2. Record a codec go/no-go decision and package/version contract.
  Requires VT.3.1 and the codec lane of VTCode #88.
  - See design sections 2 and 8.
  - Success: adoption requires no lossy fields, forbidden dependencies, or
    regression in strict completion checks; rejection keeps native delivery
    moving and records why the candidate failed.
- [ ] VT.3.3. Implement the narrow codec conversion adapter and retire only
  the duplicate implementation it replaces. Requires a go decision in VT.3.2.
  - See design section 6 and streaming design sections 4 and 5.
  - Success: Axinite owns normalized application values, policy, and admission;
    all existing streaming contracts and negative controls still pass.

## 4. Consider selected provider adapters separately

Objective: reuse transport implementations only when ownership stays explicit.

Learning opportunity: measure whether additional provider reach justifies the
conversion, dependency, and upgrade costs.

- [ ] VT.4.1. Evaluate one headless adapter from VTCode #86 with host-resolved
  endpoint, credentials, capabilities, and diagnostics. Requires VT.1.3,
  streaming tasks 3.2.5.1-3.2.5.3, and the selected adapter's #85 boundary.
  - See design section 6; coordinate existing VTCode #41, #42, and #46.
  - Success: local mocks prove ID/usage/error/cancellation fidelity and no
    ambient login, config discovery, tool execution, or hidden retries.
- [ ] VT.4.2. Decide policy ownership and import through the existing Axinite
  provider boundary. Requires VT.4.1 and a provider-specific go decision.
  - See design section 6.
  - Success: exactly one layer owns retries, admission, deadlines, and budgets;
    no competing archive or continuation store appears; negative controls
    detect duplicate retries and configuration-insensitive capability caches.
- [ ] VT.4.3. Prove recovery and operational parity, then enable the selected
  adapter behind an independent switch. Requires VT.4.2, streaming 3.2.6.4,
  and the provider lane of VTCode #88.
  - See design section 8.
  - Success: both database backends retain conservative recovery; the native
    path remains available; rollback occurs only at a safe response boundary.

## 5. Maintain the component boundary

Objective: prevent upgrades from silently reintroducing the original coupling.

Learning opportunity: observe real maintenance and build costs after adoption.

- [ ] VT.5.1. Add change-scoped upgrade checks, provenance records, and
  ownership for every adopted component. Requires the corresponding import.
  - See design sections 2, 7, and 8; coordinate VTCode #88.
  - Success: dependency, fixture, licence, and feature changes are explicit in
    upgrades; failing contracts block promotion; normal CI uses no paid calls.
- [ ] VT.5.2. Rehearse rollback and decide whether to expand, retain, or remove
  each import using measured evidence. Requires VT.5.1.
  - See design section 8.
  - Success: plain UI and native provider alternatives remain usable within the
    documented rollback window; no whole-agent migration follows implicitly.

UI and codec lanes can proceed in parallel. Provider import is optional. VTCode
#47 and full #50 completion are not blanket prerequisites. This design PR adds
no Cargo dependencies or runtime flags and changes no feature-parity status.
