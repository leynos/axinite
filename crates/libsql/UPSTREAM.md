# libSQL transport source patch

This directory contains the published `libsql` 0.9.30 SDK under its original
MIT licence. Its crate archive SHA-256 is
`30fe980ac5693ed1f3db490559fb578885e913a018df64af8a1a46e1959a78df`. The archive
was checked against the checksum in the original Axinite lockfile before
extraction. The crate archive omitted the licence text; `LICENSE` was retrieved
from
[the upstream repository at the archive's VCS revision 0653c5788d77ef16a97c56ff3e9fdc11717a72d9](https://raw.githubusercontent.com/tursodatabase/libsql/0653c5788d77ef16a97c56ff3e9fdc11717a72d9/LICENSE.md).
The dependency's stored-data format and public builder API are preserved.

The runtime patch changes only the TLS feature dependencies, the private
connector factory, and the added transport module. Test dependencies and an
explicit standalone workspace support its loopback contracts. Seven elided
lifetimes are also written explicitly to resolve compiler warnings without
changing their meaning or the public API. Source comments were corrected to the
repository's spelling conventions, and private identifiers with spelling errors
were corrected without changing the public API. Published API names such as
`TransactionBehavior`, `transaction_with_behavior`, `DropBehavior`,
`drop_behavior`, and `set_drop_behavior` retain their spelling. So do SQLite
and wire-protocol names such as `ANALYZE`, `AuthAction::Analyze`, and
`wal_flavor`. The connector uses patched rustls 0.23 and Tokio TLS with
libSQL's existing Hyper 0.14 socket boundary. Native roots, certificate and
hostname validation, HTTP/HTTPS routing, empty ALPN and TCP_NODELAY remain
enabled. Usable native roots are retained when other entries are malformed or
partly fail to load; a store without usable roots returns an error. The
application does not acquire a direct legacy Hyper dependency; the existing h2
audit ownership guard remains unchanged.

The root and fuzz manifests select this source through `[patch.crates-io]`. The
SDK is excluded from Axinite's workspace member list, like an ordinary
third-party dependency. Upstream files retain their published layout and
formatting apart from repository-required README reflow and trailing-whitespace
clean-up; the added transport files are maintained by Axinite. The SDK's
published Cargo.lock was replaced with a regenerated standalone test lockfile.
The host and fuzz lockfiles resolve its production dependencies separately.

Axinite packages the SDK library and TLS contracts, without upstream's
benchmark harness. The benchmark target and its `criterion` and `pprof`
dependencies are omitted from the compiled manifest; they are unused by the
library and tests. `Cargo.toml.orig` and `benches/benchmark.rs` retain the
published benchmark source as provenance. This removes the profiling-only
`quick-xml` dependency path from the standalone test graph.

Remove this source patch and both manifest overrides when upstream ships a
compatible transport using advisory-clean TLS dependencies. Do not broaden the
patch into SDK API, replication protocol, persistence or unrelated upstream
clean-up.
