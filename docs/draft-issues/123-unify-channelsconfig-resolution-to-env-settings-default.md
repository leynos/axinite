# Unify ChannelsConfig resolution to env > settings > default

## Summary

- Source commit: `e74214dce8fe6013b8a9a8dd02fd13cacf263131`
- Source date: `2026-03-15`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: broad config, extensions/registry.

## What the upstream commit addressed

Upstream commit `e74214dce8fe6013b8a9a8dd02fd13cacf263131` (`fix(config): unify
ChannelsConfig resolution to env > settings > default (#1124)`) addresses unify
channelsconfig resolution to env > settings > default.

Changed upstream paths:

- src/config/channels.rs
- src/config/mod.rs
- src/extensions/manager.rs
- src/settings.rs

Upstream stats:

```text
 src/config/channels.rs    | 342 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-----------
 src/config/mod.rs         |   4 +-
 src/extensions/manager.rs |   3 +-
 src/settings.rs           |  54 ++++++++++++++++-
 4 files changed, 367 insertions(+), 36 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad config,
extensions/registry.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad config, extensions/registry) means the fix could
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
