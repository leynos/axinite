#!/usr/bin/env bash
# Fast capability check for the repository's development standard.
#
# Runs before every build, test, lint and typecheck target so a missing tool
# produces an actionable installation hint rather than an opaque linker or
# compiler failure deep inside a Cargo invocation. Exits non-zero when a required
# component is absent, unusable, or does not match its pin. It never falls back
# to a slower or different build shape.

set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/build-tools-common.sh
. "$script_dir/build-tools-common.sh"

# Report on the linker half of the prerequisites. Returns non-zero when mold or
# its compiler driver is required but missing, unusable, or a different version
# from the pin. Only a non-Linux host is tolerated, with a note explaining what
# is used instead.
check_mold() {
  local pinned=$1 installed resolved
  if ! is_linux; then
    note "mold is Linux-only; falling back to the default $(uname -s) linker"
    return 0
  fi
  # `.cargo/config.toml` names clang as the linker for x86_64 Linux: the
  # `-fuse-ld=mold` flag is a compiler-driver flag that an older system `cc`
  # does not understand.
  if ! command -v clang >/dev/null 2>&1; then
    note 'clang not found on PATH; .cargo/config.toml names it as the Linux linker'
    note 'install it with your package manager (for example: apt-get install clang)'
    return 1
  fi
  if ! resolved=$(command -v mold 2>/dev/null); then
    note "mold not found on PATH (pinned $pinned)"
    note 'install it with: make install-build-tools'
    return 1
  fi
  # A mold that cannot report its version is broken -- a truncated download or
  # an unresolved shared library -- so treat it as a failure rather than letting
  # the empty string surface as a confusing version-drift message.
  if ! installed=$(installed_mold_version) || [ -z "$installed" ]; then
    note "mold at $resolved is on PATH but cannot report its version"
    note 'reinstall it with: make install-build-tools'
    return 1
  fi
  # A drift from the pin fails rather than warns: an advisory pin is not a pin.
  # `make install-build-tools` puts the pinned release ahead of a distribution
  # one on PATH, so the remedy is a single command. Report the resolved path
  # too, because `-fuse-ld=mold` selects by PATH order.
  if [ "$installed" != "$pinned" ]; then
    note "mold $installed at $resolved does not match the pin $pinned"
    note 'run make install-build-tools to match'
    return 1
  fi
  note "mold $installed at $resolved"
}

# Whether a component the pin lists is installed on the toolchain. `rustup
# component list` names a component by its dist name with the target appended
# and `-preview` dropped, so `llvm-tools-preview` is `llvm-tools-<triple>`.
component_installed() {
  local toolchain=$1 component=$2 installed
  installed=$(rustup component list --toolchain "$toolchain" --installed 2>/dev/null) ||
    return 1
  printf '%s\n' "$installed" | grep -q "^${component%-preview}-"
}

# Report on the toolchain half of the prerequisites: rustup itself, the pinned
# nightly, every component the pin lists and every target.
# Any absence is fatal: the parallel frontend is nightly-only, and a missing
# component fails deep inside Cargo with an opaque message.
check_toolchain() {
  local toolchain=$1 component target status=0 targets installed_toolchains rustup_state host installed_name
  if ! command -v rustup >/dev/null 2>&1; then
    note 'rustup not found on PATH; it is required to select the pinned nightly'
    note 'install it from https://rustup.rs'
    return 1
  fi
  if ! installed_toolchains=$(rustup toolchain list 2>/dev/null); then
    note 'rustup could not list installed toolchains'
    return 1
  fi
  if ! rustup_state=$(rustup show 2>/dev/null); then
    note 'rustup could not report the host triple'
    return 1
  fi
  host=$(awk -F': ' '$1 == "Default host" { print $2; exit }' <<< "$rustup_state")
  if [ -z "$host" ]; then
    note 'rustup did not report the default host triple'
    return 1
  fi
  while IFS= read -r installed_name; do
    installed_name=${installed_name%%[[:space:]]*}
    if [ "$installed_name" = "$toolchain" ] ||
      [ "$installed_name" = "$toolchain-$host" ]; then
      break
    fi
    installed_name=
  done <<< "$installed_toolchains"
  if [ -z "$installed_name" ]; then
    note "toolchain $toolchain is not installed for host $host"
    note 'install it with: make install-build-tools'
    return 1
  fi
  while IFS= read -r component; do
    [ -n "$component" ] || continue
    if ! component_installed "$toolchain" "$component"; then
      note "component $component is missing from toolchain $toolchain"
      note 'install it with: make install-build-tools'
      status=1
    fi
  done < <(toolchain_components)
  targets=$(rustup target list --toolchain "$toolchain" --installed 2>/dev/null) || targets=
  while IFS= read -r target; do
    [ -n "$target" ] || continue
    if ! printf '%s\n' "$targets" | grep -qx "$target"; then
      note "target $target is missing from toolchain $toolchain"
      note 'install it with: make install-build-tools'
      status=1
    fi
  done < <(toolchain_targets)
  [ "$status" -ne 0 ] || note "toolchain $toolchain available with its components and targets"
  return "$status"
}

# Run both checks unconditionally so a developer sees every missing piece in one
# pass rather than fixing them one failed run at a time.
main() {
  local status=0 mold_pin toolchain_pin
  # Resolve the pins into variables first. `fail` exits, but inside a command
  # substitution that exit kills only the subshell, so passing `$(mold_version)`
  # straight into a check would continue with an empty pin and report a
  # nonsensical drift. An assignment propagates the status, so this stops.
  mold_pin=$(mold_version) || return 1
  toolchain_pin=$(pinned_toolchain) || return 1
  check_mold "$mold_pin" || status=1
  check_toolchain "$toolchain_pin" || status=1
  [ "$status" -eq 0 ] || note 'capability check failed; see the messages above'
  return "$status"
}

main "$@"
