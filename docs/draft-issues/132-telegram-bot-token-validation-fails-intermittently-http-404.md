# Telegram bot token validation fails intermittently (HTTP 404)

## Summary

- Source commit: `81724cad93d2eeb8aa632ee8b23ab1d43c99d0c2`
- Source date: `2026-03-15`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad CI/release, .gitignore.

## What the upstream commit addressed

Upstream commit `81724cad93d2eeb8aa632ee8b23ab1d43c99d0c2` (`fix: Telegram bot
token validation fails intermittently (HTTP 404) (#1166)`) addresses telegram
bot token validation fails intermittently (http 404).

Changed upstream paths:

- .github/workflows/e2e.yml
- .gitignore
- src/extensions/manager.rs
- tests/e2e/scenarios/test_telegram_token_validation.py

Upstream stats:

```text
 .github/workflows/e2e.yml                             |   2 +-
 .gitignore                                            |   6 +++
 src/extensions/manager.rs                             |  43 ++++++++++++++++++--
 tests/e2e/scenarios/test_telegram_token_validation.py | 172 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 4 files changed, 219 insertions(+), 4 deletions(-)
 create mode 100644 tests/e2e/scenarios/test_telegram_token_validation.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad CI/release,
.gitignore.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad CI/release, .gitignore) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
