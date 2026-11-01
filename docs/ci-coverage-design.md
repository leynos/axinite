# CI coverage publication and ratchet design

The coverage workflows use the shared CV-005 contract to keep publication on
`main` and keep pull-request measurements comparable to its baseline. The
contract configuration is in `.github/cv005.toml`;
`make test-workflow-contracts` runs the pinned shared checks followed by
repository-specific contracts.

## Ownership and trust boundaries

`coverage.yml` owns publication. Its `coverage-libsql` job generates the
libSQL-only report, ratchets it, saves the baseline on a push to `main`, and
uploads to CodeScene. It alone declares the protected `codescene` environment.
That environment admits `main` and holds `CS_ACCESS_TOKEN`. A token check
precedes the upload, which requires both the check's output and the `main` ref;
the token is passed only as the uploader's `access-token` input. The workflow
has read-only contents permission, and publisher checkouts do not retain Git
credentials. Its concurrency group is shared by runs on the same workflow and
ref and does not cancel an active run.

`codescene-coverage.yml` runs the pull-request ratchet without publishing an
artefact or receiving a CodeScene token. The publisher and ratchet use the same
toolchain, nextest profile, watchdog, generator revision, and coverage
selection. Local parity contracts detect drift, while the shared CV-005 checks
inspect pull-request reachable workflows and enforce the publication boundary.

## Report hand-off and failure handling

The libSQL report is retained as a one-day artefact. The separate
`codecov-libsql` job downloads it and uses OpenID Connect (OIDC) to publish the
`libsql-only` flag to Codecov. This keeps OIDC permission outside the protected
publisher job. The coverage roll-up checks the result of each coverage and
compile-contract job when its prerequisites succeed. The pinned CV-005 contract
forbids a job condition on the roll-up, so a failed prerequisite skips it and
fails the workflow directly.

The publisher's baseline and CodeScene upload are proved by a push to `main`:
confirm the uploader accepts the report, the baseline is saved, and Codecov
receives the libSQL report. A dispatch can upload on `main`, but the generator
saves its baseline only on a push. Automerged Dependabot changes do not emit a
push event; a later ordinary push is needed to refresh their baseline. See
[CodeScene coverage publication](developers-guide.md#codescene-coverage-publication)
for operator details and the environment read-back requirements.
