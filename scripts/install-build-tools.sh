#!/usr/bin/env bash
# Install the build tools the repository's development standard needs.
#
# Downloads the pinned mold release, verifies it against tools/mold/SHA256SUMS,
# unpacks it under $BUILD_TOOLS_PREFIX (default ~/.local), then installs the
# pinned nightly toolchain with the components and targets rust-toolchain.toml
# lists. Nothing here touches the release, packaging, coverage or Whitaker
# toolchains.
#
# CI runs this through `make install-build-tools` before every job that compiles
# the workspace. When GITHUB_PATH is set it also puts the install prefix's
# `bin/` first on PATH for the rest of the job, so direct `cargo` steps resolve
# the pinned mold as `make` targets do.

set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/build-tools-common.sh
. "$script_dir/build-tools-common.sh"

MOLD_RELEASE_BASE_URL=${MOLD_RELEASE_BASE_URL:-https://github.com/rui314/mold/releases/download}

# Download bounds, overridable so a test can drive them without waiting.
CURL_CONNECT_TIMEOUT=${CURL_CONNECT_TIMEOUT:-15}
CURL_MIN_BYTES_PER_SECOND=${CURL_MIN_BYTES_PER_SECOND:-1024}
CURL_STALL_SECONDS=${CURL_STALL_SECONDS:-60}

# The scratch directory the EXIT trap removes. A trap whose action is a string
# is re-parsed by the shell when it fires, so a path containing a quote -- from
# a quote-bearing `TMPDIR`, say -- breaks the quoting and the cleanup never
# runs. Naming a function instead means the path is only ever a variable. It is
# script-scope rather than local because the trap fires after `install_mold` has
# returned.
BUILD_TOOLS_WORKDIR=

remove_workdir() {
  [ -n "$BUILD_TOOLS_WORKDIR" ] || return 0
  rm -rf -- "$BUILD_TOOLS_WORKDIR"
  BUILD_TOOLS_WORKDIR=
}

trap remove_workdir EXIT

# Verify the downloaded tarball against the single matching line in
# SHA256SUMS. An unlisted artefact is a hard failure, never a silent skip.
verify_mold_archive() {
  local archive=$1 name=$2 expected recorded
  expected=$(awk -v name="$name" '$2 == name { print $1 }' "$MOLD_SHA256SUMS_FILE")
  [ -n "$expected" ] || fail "no checksum recorded for $name in $MOLD_SHA256SUMS_FILE"
  # Refuse an ambiguous file rather than guessing: with several rows for one
  # artefact the verdict would rest on whichever digest `sha256sum` read last.
  recorded=$(printf '%s\n' "$expected" | grep -c .)
  [ "$recorded" -eq 1 ] ||
    fail "$recorded checksums recorded for $name in $MOLD_SHA256SUMS_FILE; refusing to guess"
  printf '%s  %s\n' "$expected" "$archive" | sha256sum --check --status ||
    fail "checksum mismatch for $name; refusing to install"
  note "verified $name against $MOLD_SHA256SUMS_FILE"
}

# Download, verify and unpack the pinned mold release, or explain why the step
# is being skipped on a platform mold does not support.
install_mold() {
  local version=$1 arch name url workdir
  if ! is_linux; then
    note "mold is Linux-only; skipping on $(uname -s), the platform linker is used instead"
    return 0
  fi
  arch=$(mold_arch)
  name="mold-$version-$arch-linux.tar.gz"
  url="$MOLD_RELEASE_BASE_URL/v$version/$name"

  BUILD_TOOLS_WORKDIR=$(mktemp -d)
  workdir=$BUILD_TOOLS_WORKDIR

  note "downloading $url"
  # Bound both the handshake and the transfer. Without these a server that
  # accepts the connection and then stalls leaves `curl` waiting indefinitely.
  # The transfer bound is a stall detector rather than a deadline, so a slow
  # but progressing link is not punished.
  curl --fail --silent --show-error --location \
    --connect-timeout "$CURL_CONNECT_TIMEOUT" \
    --speed-limit "$CURL_MIN_BYTES_PER_SECOND" --speed-time "$CURL_STALL_SECONDS" \
    --output "$workdir/$name" "$url" ||
    fail "failed to download $name"
  verify_mold_archive "$workdir/$name" "$name"

  # The tarball root is mold-<version>-<arch>-linux/{bin,lib,libexec}; strip it
  # so the tree merges into the prefix and `bin/ld.mold` lands on PATH.
  mkdir -p "$BUILD_TOOLS_PREFIX"
  tar --extract --gzip --strip-components=1 --directory "$BUILD_TOOLS_PREFIX" --file "$workdir/$name" ||
    fail "failed to unpack $name into $BUILD_TOOLS_PREFIX"
  note "installed mold $version into $BUILD_TOOLS_PREFIX"
  export_path_to_ci
}

# Put the install prefix first on PATH for the later steps of a CI job. The
# Makefile prepends the same prefix for its own targets; this covers the direct
# `cargo` steps, which would otherwise find a distribution mold first or none.
export_path_to_ci() {
  if [ -n "${GITHUB_PATH:-}" ]; then
    printf '%s\n' "$BUILD_TOOLS_PREFIX/bin" >>"$GITHUB_PATH"
    note "added $BUILD_TOOLS_PREFIX/bin to GITHUB_PATH"
  else
    note "put $BUILD_TOOLS_PREFIX/bin first on PATH when not using the make targets"
  fi
}

# Install the pinned nightly with the components and targets the pin lists.
# Uses the minimal profile so the pin's own list is the whole set. The same
# toolchain rust-toolchain.toml selects, installed eagerly so a missing one is
# reported here rather than mid-build.
install_toolchain() {
  local toolchain=$1 component target
  local -a args=()
  command -v rustup >/dev/null 2>&1 ||
    fail 'rustup not found on PATH; install it from https://rustup.rs'
  while IFS= read -r component; do
    [ -n "$component" ] && args+=(--component "$component")
  done < <(toolchain_components)
  while IFS= read -r target; do
    [ -n "$target" ] && args+=(--target "$target")
  done < <(toolchain_targets)
  note "installing toolchain $toolchain"
  rustup toolchain install "$toolchain" --profile minimal "${args[@]}" ||
    fail "failed to install toolchain $toolchain"
}

# Install both halves. The linker step runs first so a checksum failure aborts
# before spending time on a toolchain download.
main() {
  local mold_pin toolchain_pin
  # Resolve the pins into variables first: `fail` exits, but inside a command
  # substitution that exit kills only the subshell, so an unreadable pin would
  # otherwise reach the installer as an empty string and be built into a
  # download URL.
  mold_pin=$(mold_version) || return 1
  toolchain_pin=$(pinned_toolchain) || return 1
  install_mold "$mold_pin"
  install_toolchain "$toolchain_pin"
  note 'ready; verify with: make check-build-tools'
}

main "$@"
