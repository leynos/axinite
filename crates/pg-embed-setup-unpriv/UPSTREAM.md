# `pg-embed-setup-unpriv` source patch

This directory carries the published `pg-embed-setup-unpriv` 0.6.3 crate. Its
registry archive SHA-256 is
`4138a96b28dda86101e396f6c3e6ed475dcc4c0205ba3206d463be5d9c3959e6`, and its
upstream VCS revision is `5d43ecf816a9c1404f78d1130699f2520605441d`. The
upstream ISC licence and package source are retained.

The slot claim now holds `.claim-guard` across both dead-slot sweeping and
creation of the next slot. Before this change, a concurrent bootstrap could
open and lock a newly created `.lock` file during the gap between its creation
and the owner's lock operation. The owner then failed with “slot lock is
already held”, and the shared test-cluster singleton cached that bootstrap
failure for the process. The regression test pauses the first claim in its
sweep and verifies that a second claim waits for the guard.

The `cfg_aliases` build dependency is replaced by direct Cargo cfg emission.
Its macro expansion triggered a future-incompatible trailing-semicolon warning
on current toolchains. The build script registers `privileged_unix_platform`
and emits it for Linux, Android, FreeBSD, OpenBSD, and DragonFly targets
without suppressing the warning.

Repeated cache, cleanup, extension, startup-lock, password-tracing, and
singleton model-test setup is consolidated into shared fixtures and case
tables. Template rollback also uses one cleanup path while retaining the
distinction between returned setup errors and resumed panics. These local
refactors preserve the upstream test scenarios and failure behaviour.

This local source patch is selected through the root `[patch.crates-io]`
section. Remove it when an upstream release includes the guarded sweep and
claim sequence, and emits the platform cfg without compiler warnings.
