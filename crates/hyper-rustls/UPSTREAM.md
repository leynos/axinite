# hyper-rustls transport source patch

This directory contains the published `hyper-rustls` 0.25.0 crate. Its archive
SHA-256 is `399c78f9338483cb7e630c8474b07268983c6bd5acee012e4211f9f7bb21b070`,
verified against the original lockfile. The archive's VCS revision is
`def00ca3d4d9dca46bfde5574f4b0f3d2b722a74`. The upstream Apache-2.0, ISC and
MIT license files are retained.

The root and fuzz manifests select this source through `[patch.crates-io]`.
This non-publishable crate is a member of the root workspace and shares its
`Cargo.lock`; it adds no standalone audited graph. The patch updates TLS
dependencies to rustls 0.23.46 or later, tokio-rustls 0.26.4 or later, and
rustls-native-certs 0.8. Native-root loading retains valid certificates when
other entries are malformed or fail to load, and returns `NotFound` when no
usable roots remain. The ring native-root shorthand selects its provider
explicitly without installing a process-global provider. The existing infallible
`with_webpki_roots` method continues to use rustls' process-default provider;
callers can choose a provider explicitly with `with_provider_and_webpki_roots`.

The libSQL SDK continues to use its native-root connector shorthand. Its public
API, stored-data format and replication protocol remain unchanged. The SDK's
HTTP/HTTPS routing, certificate and hostname verification, HTTP/1 empty ALPN
configuration and TCP_NODELAY remain in effect. This patch does not add a
direct Hyper dependency to the application. Connector builder types such as
`ClientConfig` and `CryptoProvider` come from rustls 0.23.

The patch centralizes lazy server handshakes and uses Tokio's `Either` I/O
adapter from `tokio-util` for stream forwarding. Public stream variants and
connection metadata are retained; the adapter adds no heap allocation.

The existing h2 audit exception remains owned by the libSQL dependency path;
the audit path guard and its lockfile graph set are unchanged.

The HTTPS-only connector rejects plaintext HTTP URIs before opening a socket;
the libSQL SDK continues to select its existing `https_or_http` mode,
preserving its HTTP and HTTPS routing.

The upstream `tests/tests.rs` is not carried forward. It launches prebuilt
example binaries, contacts Google, reserves fixed ports, and invokes `curl`
with certificate verification disabled. The replacement coverage uses local
loopback connections and generated certificates for HTTP routing, TLS
verification, host names, ALPN/HTTP versions, and isolated provider selection;
it requires neither public network access nor an external command-line client.

The examples parse PEM inputs through the maintained `rustls-pki-types` API,
preserving explicit caller-selected file access and underlying I/O errors. The
unmaintained `rustls-pemfile` helper is no longer required.

Run `make test-libsql-transport` for the all-features loopback transport tests,
including HTTP/2, and the PEM example tests. Workspace nextest also includes
the crate's default HTTP/1 TLS tests. The dedicated target runs once in its own
CI lane. `make lint-libsql-transport` covers its all-targets/all-features
Clippy lane; the workspace lint lanes also cover the member. `make audit`
continues to check the existing 18 lockfile graphs, including root and fuzz
resolutions.

Remove this source patch and both manifest overrides when the upstream libSQL
SDK updates its TLS dependencies to advisory-clean versions. Keep any future
patch limited to the dependency boundary; do not fork SDK behaviour or its
public API.
