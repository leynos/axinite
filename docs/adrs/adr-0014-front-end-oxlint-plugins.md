# Architectural decision record (ADR) 0014: Apply Oxlint plug-ins to the front end

## Status

Accepted. The front-end lint gate retains Biome and adds pinned Oxlint Unicorn
and `ai-deslop` rule sets, with only documented policy conflicts disabled.

## Date

2026-10-20.

## Context and problem statement

The `web-src` workspace uses Biome for formatting and general-purpose linting,
with separate TypeScript, semantic, CSS, accessibility, and test gates. It did
not run Oxlint. This left the front end without Unicorn's recommended rules or
the evidence-focused rules in `ai-deslop`.

Unicorn is built into Oxlint, while `ai-deslop` is loaded as an Oxlint
JavaScript plug-in. Oxlint's built-in and JavaScript plug-in versions must
remain compatible, and the enabled rules need a stable, reviewable profile.

## Decision Drivers

- Biome remains the owner of formatting and its existing general lint rules.
- The requested Unicorn and `ai-deslop` recommendations must run on front-end
  source, tests, mock-backend code, and lint scripts.
- Exact tool versions and explicit rules keep the result reproducible.
- Existing API nullability and the preference for Unicorn over conflicting
  `ai-deslop` rules must remain clear to future maintainers.

## Requirements

- Pin Oxlint, `ai-deslop`, and the type-aware backend to exact versions.
- Enable every Unicorn rule in Oxlint's supported recommended profile,
  except where an existing documented contract conflicts.
- Enable the `ai-deslop` recommended rules, except where one directly
  contradicts an enabled Unicorn rule.
- Keep existing Biome, TypeScript, CSS, semantic, and test gates in place.
- Do not disable additional rules to accommodate existing lint failures;
  repair the code that violates enabled rules.

## Options considered

### Option A: enable Oxlint category presets

This is concise, but the category sets are not the Unicorn recommended profile
and can change independently of the plug-in rules.

### Option B: pin the toolchain and enumerate the requested profiles

This pins the supported Unicorn and `ai-deslop` rules individually. It makes
the enabled set and each exception visible in configuration and review.

### Option C: replace Biome with Oxlint

This would remove the existing formatter and general-purpose lint owner, and
would not preserve the current front-end quality gates.

| Topic                   | Option A | Option B | Option C |
| ----------------------- | -------- | -------- | -------- |
| Profile stability       | Low      | High     | Medium   |
| Exception visibility    | Low      | High     | Medium   |
| Preserves current gates | Yes      | Yes      | No       |

_Table 1: Comparison of the front-end linting options._

## Decision outcome

Option B. The `web-src` development dependencies pin the lint toolchain:

| Component         | Version    | Role                                        |
| ----------------- | ---------- | ------------------------------------------- |
| `oxlint`          | `1.82.0`   | Linter and built-in Unicorn implementation  |
| `unicorn`         | Built in   | Supported Unicorn recommended rules         |
| `ai-deslop`       | `0.4.8`    | Deslop recommended JavaScript plug-in rules |
| `oxlint-tsgolint` | `7.0.2004` | Type-aware Oxlint backend                   |
| TypeScript 7      | `7.0.2`    | Native compiler for typecheck and Oxlint    |
| TypeScript 6      | `6.0.2`    | Compatibility API for compiler-aware tools  |

_Table 2: Pinned front-end Oxlint components._

`ai-deslop` brings `@oxlint/plugins` at `1.82.0`; this matches the pinned
Oxlint version. `bun.lock` records the full dependency graph.

Oxlint's type-aware backend requires TypeScript 7. The native compiler is
installed as `@typescript/native` and is used for the frontend typecheck. The
TypeScript 6 compatibility package stays installed under the `typescript`
package name for dependencies that still use its JavaScript compiler API. Both
compiler versions run in the `check:types` script. Before switching the gate,
the same tree typechecked cleanly with both compilers. The TypeScript
configuration removes the deprecated `baseUrl`, makes the existing alias
project-relative, and enables the ES2023 library declarations used by Unicorn's
non-mutating array methods.

The compatibility package exposes the `tsc6` binary, which `check:types:6`
invokes for the compatibility typecheck.

`web-src/.oxlintrc.json` enumerates the supported Unicorn recommended rules and
the `ai-deslop` recommended rules. Oxlint categories remain off so that
unrequested default categories do not silently expand the Biome-owned lint
baseline. The recommended profile's rules are enabled explicitly as errors. The
configuration also enables the type-aware rules supplied with the `ai-deslop`
recommendation and sets `options.typeAware` to `true`.

The front-end `lint` script runs Oxlint on `axinite/src`, `axinite/tests`,
`mock-backend`, and `scripts`, after the existing Biome lint command.
Formatting, TypeScript checking, semantic checks, and tests remain separate
gates.

### Disabled rules and conflict handling

`unicorn/no-null` is disabled because the existing HTTP API contract represents
nullable values as JSON `null`. The web API reference shows
`pending_approval: null`, and API DTOs include explicit nullable fields such as
`completed_at: string | null` and `assistant_thread: ThreadInfo | null`.
Replacing those values with `undefined` would omit fields during JSON
serialization and change the wire representation.

`deslop/no-reflect-apply` is disabled because it directly contradicts
`unicorn/prefer-reflect-apply`. Unicorn takes precedence as requested. Other
overlaps are resolved by changing the call site to satisfy both rules.

No other Unicorn or `ai-deslop` rules are disabled. Future exceptions require a
conflict with existing documentation, Biome or `df12-lints` requirements,
TypeDoc output, or a direct conflict between the two plug-ins; Unicorn prevails
in the last case.

## Known risks and limitations

- Oxlint JavaScript plug-ins are an alpha interface. The exact pins keep the
  current gate stable; upgrades must validate compatibility and rerun the
  front-end gates.
- The new recommended rules expose existing lint debt. The code must be
  repaired rather than hidden behind broader disables or a baseline.

## Architectural rationale

The change keeps one owner for each existing concern: Biome remains the
formatter and general-purpose linter, TypeScript owns static types, and Oxlint
adds the requested syntax-aware Unicorn and evidence-focused `ai-deslop` rules.
Explicit rules and exact pins keep that boundary auditable.

## Addendum (2026-10-22): Bun dependency updates

The repository configuration added in main commit `c187e632` (#446) now runs
daily Dependabot updates for `/web-src`, grouping minor and patch updates and
applying the shared seven-day default and fourteen-day major-version cooldowns.
This policy covers the pinned Oxlint and `ai-deslop` dependencies as well as
the rest of the front-end dependency graph. No additional update configuration
is needed here; proposed version changes must still pass the compatibility and
front-end gates described in this decision.

## Addendum (2026-10-24): Follow the ADR index convention

This record follows the repository's current ADR index convention: store
records in `docs/adrs/`, use four-digit sequence numbers, and link each record
from `docs/contents.md`. The index now lists this decision as ADR 0014.

## Addendum (2026-10-24): Pin an available TypeScript 6 release

The npm registry publishes `@typescript/typescript6` 6.0.2; 6.0.3 is
unavailable. Pin the `typescript` compatibility alias to 6.0.2 so a clean
install can reproduce the dual compiler checks.

## References

- [Oxlint built-in plug-ins](https://oxc.rs/docs/guide/usage/linter/plugins.html)
- [Oxlint JavaScript plug-ins](https://oxc.rs/docs/guide/usage/linter/js-plugins.html)
- [TypeScript 7 announcement](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)
- [TypeScript 6 compatibility package](https://www.npmjs.com/package/@typescript/typescript6)
- [Unicorn recommended configuration](https://github.com/sindresorhus/eslint-plugin-unicorn#recommended-config)
- [`ai-deslop` package](https://www.npmjs.com/package/ai-deslop)
- [Model pull request #121](https://github.com/leynos/simulacat-core/pull/121)
- [Repository dependency policy, pull request #446](https://github.com/leynos/axinite/pull/446)
- [Web API reference](../web-api-reference.md)
- [Front-end Oxlint configuration](../../web-src/.oxlintrc.json)
