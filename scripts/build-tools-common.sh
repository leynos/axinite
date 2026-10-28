#!/usr/bin/env bash
# Shared helpers for the build tools the repository's development standard needs.
#
# Sourced by scripts/install-build-tools.sh and scripts/check-build-tools.sh so
# both resolve the pinned versions and emit diagnostics identically. Every
# message is prefixed `build-tools:` and written to stderr.

set -euo pipefail

# Locate the repository from this file rather than from the working directory,
# so the entry points run correctly when invoked directly as well as through the
# `install-build-tools` and `check-build-tools` targets. `BASH_SOURCE[0]` is this
# file even when sourced, which is what makes the derivation reliable.
BUILD_TOOLS_HELPER_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
BUILD_TOOLS_REPO_ROOT=$(cd -- "$BUILD_TOOLS_HELPER_DIR/.." && pwd)

# Pin files default to their committed locations. An explicit override still
# wins, which is what lets tests point the scripts at fixtures. `read_pin`
# validates whichever path is selected, so a missing or empty file is reported
# the same way whether it came from a default or an override.
MOLD_VERSION_FILE="${MOLD_VERSION_FILE:-$BUILD_TOOLS_REPO_ROOT/tools/mold/VERSION}"
MOLD_SHA256SUMS_FILE="${MOLD_SHA256SUMS_FILE:-$BUILD_TOOLS_REPO_ROOT/tools/mold/SHA256SUMS}"
RUST_TOOLCHAIN_FILE="${RUST_TOOLCHAIN_FILE:-$BUILD_TOOLS_REPO_ROOT/rust-toolchain.toml}"

# Prefix for the mold installation tree. The Makefile prepends this prefix's
# `bin/` to PATH for every target -- this exact prefix, not a hard-coded
# ~/.local -- so an overridden BUILD_TOOLS_PREFIX is the one that wins PATH
# resolution for both `check-build-tools` and `-fuse-ld=mold`. Invoking these
# scripts outside `make` means arranging that PATH order separately.
BUILD_TOOLS_PREFIX="${BUILD_TOOLS_PREFIX:-$HOME/.local}"

# Emit a diagnostic. Always stderr, so a caller may capture a helper's stdout
# without the diagnostics contaminating the captured value.
note() { printf 'build-tools: %s\n' "$*" >&2; }

# Emit a diagnostic and abort. Used for conditions no caller can recover from,
# such as a missing pin file or an unverifiable download.
fail() {
  printf 'build-tools: %s\n' "$*" >&2
  exit 1
}

# Read a single-line version pin, trimming only its leading and trailing
# whitespace.
#
# A missing, blank, or multi-line pin aborts rather than yielding a version that
# would silently produce a nonsensical download URL. Deleting every whitespace
# character instead would corrupt rather than reject: a stray space would turn
# `1.2 3` into `1.23`, and a second line would concatenate into `1.2.34.5.6`.
read_pin() {
  local file=$1 value lines
  [ -f "$file" ] || fail "missing version pin: $file"
  lines=$(grep -c '' <"$file")
  [ "$lines" -le 1 ] || fail "expected one line in version pin: $file, found $lines"
  value=$(cat -- "$file")
  value=${value#"${value%%[![:space:]]*}"}
  value=${value%"${value##*[![:space:]]}"}
  [ -n "$value" ] || fail "empty version pin: $file"
  case $value in
    *[[:space:]]*) fail "version pin contains whitespace: $file" ;;
  esac
  printf '%s' "$value"
}

# The pinned mold release tag, e.g. "2.41.0".
mold_version() { read_pin "$MOLD_VERSION_FILE"; }

# The repository's toolchain channel, read from `rust-toolchain.toml`.
#
# Deliberately the toolchain every ordinary gate uses, not a second pin: the
# parallel frontend the standard uses is nightly-only.
pinned_toolchain() {
  local file=$RUST_TOOLCHAIN_FILE value
  [ -f "$file" ] || fail "missing version pin: $file"
  value=$(awk -F'"' '/^[[:space:]]*channel[[:space:]]*=/ { print $2; exit }' "$file")
  [ -n "$value" ] || fail "no channel found in: $file"
  printf '%s' "$value"
}

# Read and validate one non-empty array of simple quoted names in the
# `[toolchain]` table, returning one name per line. Rustup's toolchain pin uses
# component and target identifiers, so accepting any other TOML value would
# make the installer silently omit part of the requested toolchain.
toolchain_list() {
  local key=$1 file=$RUST_TOOLCHAIN_FILE
  [ -f "$file" ] || fail "missing version pin: $file"
  case $key in
    components | targets) ;;
    *) note "unsupported toolchain array: $key"; return 2 ;;
  esac
  awk -v key="$key" '
    function trim(value) {
      sub(/^[[:space:]]+/, "", value)
      sub(/[[:space:]]+$/, "", value)
      return value
    }
    function without_comment(value,    result, i, char, quoted, escaped) {
      result = ""
      quoted = 0
      escaped = 0
      for (i = 1; i <= length(value); i++) {
        char = substr(value, i, 1)
        if (char == "#" && !quoted) break
        result = result char
        if (char == "\"" && !escaped) quoted = !quoted
        if (char == "\\" && quoted) escaped = !escaped
        else escaped = 0
      }
      return result
    }
    function invalid(reason) {
      if (!failed) {
        printf "build-tools: invalid %s array in %s: %s\n", key, FILENAME, reason > "/dev/stderr"
      }
      failed = 1
      exit 2
    }
    function parse_array_line(value,    char, token) {
      value = trim(without_comment(value))
      if (value == "") return

      if (state == "opening") {
        if (substr(value, 1, 1) != "[") invalid("expected opening bracket")
        value = substr(value, 2)
        state = "value"
      }

      while (value != "") {
        value = trim(value)
        if (value == "") return
        char = substr(value, 1, 1)

        if (state == "value") {
          if (char == "]") {
            if (count == 0) invalid("array must not be empty")
            state = "closed"
            value = trim(substr(value, 2))
            if (value != "") invalid("unexpected text after closing bracket")
            return
          }
          if (char != "\"") invalid("expected a quoted component or target name")
          if (!match(value, /^"[A-Za-z0-9_][A-Za-z0-9_-]*"/)) {
            invalid("array entries must be non-empty quoted component or target names")
          }
          token = substr(value, 2, RLENGTH - 2)
          print token
          count++
          value = substr(value, RLENGTH + 1)
          state = "separator"
          continue
        }

        if (state == "separator") {
          if (char == ",") {
            value = substr(value, 2)
            state = "value"
            continue
          }
          if (char == "]") {
            state = "closed"
            value = trim(substr(value, 2))
            if (value != "") invalid("unexpected text after closing bracket")
            return
          }
          invalid("expected comma or closing bracket after an array entry")
        }
      }
    }
    BEGIN { section = ""; state = "missing"; count = 0; failed = 0 }
    {
      line = trim(without_comment($0))
      if (line ~ /^\[[[:alnum:]_.-]+\]$/) {
        if (state == "opening" || state == "value" || state == "separator") {
          invalid("unterminated array before the next table")
        }
        section = line
        next
      }

      if (state == "opening" || state == "value" || state == "separator") {
        if (line ~ /^[[:alpha:]_][[:alnum:]_-]*[[:space:]]*=/) {
          invalid("unterminated array before the next key")
        }
      }

      if (section != "[toolchain]") next
      if (state == "closed") {
        if (line ~ ("^" key "[[:space:]]*=")) invalid("duplicate key")
        next
      }
      if (state == "missing") {
        if (line ~ ("^" key "[[:space:]]*=")) {
          sub("^" key "[[:space:]]*=", "", line)
          state = "opening"
          parse_array_line(line)
        }
        next
      }
      parse_array_line($0)
    }
    END {
      if (failed) exit 2
      if (state == "missing") {
        printf "build-tools: invalid %s array in %s: key is missing from [toolchain]\n", key, FILENAME > "/dev/stderr"
        exit 2
      }
      if (state != "closed") {
        printf "build-tools: invalid %s array in %s: unterminated array\n", key, FILENAME > "/dev/stderr"
        exit 2
      }
    }
  ' "$file"
}

# Components and targets the pin asks for.
toolchain_components() { toolchain_list components; }
toolchain_targets() { toolchain_list targets; }

# Whether the host can use mold at all; it ships for Linux only.
is_linux() { [ "$(uname -s)" = 'Linux' ]; }

# mold publishes per-architecture tarballs; map `uname -m` onto those names.
mold_arch() {
  local machine
  machine=$(uname -m)
  case "$machine" in
    x86_64 | amd64) printf 'x86_64' ;;
    aarch64 | arm64) printf 'aarch64' ;;
    *) fail "unsupported architecture for the pinned mold release: $machine" ;;
  esac
}

# The version of whichever mold PATH resolves to, or a non-zero status when
# that mold cannot be run.
installed_mold_version() {
  # `mold --version` prints e.g. "mold 2.41.0 (compatible with GNU ld)". Capture
  # first so a failing mold propagates its status instead of being masked by the
  # exit status of a downstream awk.
  local output
  output=$(mold --version 2>/dev/null) || return 1
  printf '%s' "$output" | awk 'NR == 1 { print $2 }'
}
