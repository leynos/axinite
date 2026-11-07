"""Forward Make's prover settings as literal arguments to rust-prover-tools."""

from __future__ import annotations

import os
import shlex
import subprocess
import sys
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path

DEFAULT_KANI_VERSION_FILE = "tools/kani/VERSION"


def _required(name: str, value: str) -> str:
    """Return ``value``, raising when the setting is unset or empty.

    Parameters
    ----------
    name : str
        Setting name used in the error message.
    value : str
        Candidate value supplied by the caller.

    Returns
    -------
    str
        The non-empty ``value``.

    Raises
    ------
    ValueError
        If ``value`` is empty.
    """
    if not value:
        raise ValueError(f"{name} must be configured")
    return value


def _setting(name: str) -> str:
    """Return the non-empty setting ``name``.

    Parameters
    ----------
    name : str
        Environment variable that the Make target must configure.

    Returns
    -------
    str
        The configured value.

    Raises
    ------
    ValueError
        If the setting is unset or empty.
    """
    return _required(name, os.environ.get(name, ""))


def _split(value: str, name: str) -> list[str]:
    """Split a quoted command or flag list into literal arguments.

    Parameters
    ----------
    value : str
        Quoted command line to split.
    name : str
        Setting name used in the error message.

    Returns
    -------
    list[str]
        The parsed arguments.

    Raises
    ------
    ValueError
        If ``value`` contains unmatched quoting.
    """
    try:
        return shlex.split(value)
    except ValueError as error:
        raise ValueError(f"invalid {name}: unmatched quoting") from error


def _selects_proof_file(flags: list[str]) -> bool:
    """Report whether ``flags`` already select the Verus proof file."""
    return any(
        flag == "--proof-file" or flag.startswith("--proof-file=") for flag in flags
    )


def _proof_arguments(settings: Mapping[str, str]) -> list[str]:
    """Return Verus proof arguments for explicit settings.

    Parameters
    ----------
    settings : Mapping[str, str]
        Values for the Verus settings, normally the process environment.
        ``VERUS_FLAGS`` holds the quoted flag list, and ``VERUS_PROOF_FILE``
        supplies the default proof file when the flags select none.

    Returns
    -------
    list[str]
        ``VERUS_FLAGS`` as literal arguments, preceded by the configured proof
        file when the flags do not already select one.

    Raises
    ------
    ValueError
        If ``VERUS_FLAGS`` contains unmatched quoting, or the default proof
        file is needed but ``VERUS_PROOF_FILE`` is empty.
    """
    flags = _split(settings.get("VERUS_FLAGS", ""), "VERUS_FLAGS")
    if _selects_proof_file(flags):
        return flags
    default = _required("VERUS_PROOF_FILE", settings.get("VERUS_PROOF_FILE", ""))
    return ["--proof-file", default, *flags]


def _environment_proof_arguments() -> list[str]:
    """Resolve the Verus proof arguments from the process environment."""
    return _proof_arguments(os.environ)


def _kani_installation() -> str:
    """Return the selected Kani release root.

    The Makefile leaves ``KANI_INSTALLATION`` unset so the pin is read only
    when a Kani command needs it. A missing, unreadable, or empty pin fails
    here instead of expanding to an empty version during Makefile parsing.

    Returns
    -------
    str
        The configured installation directory, or the default derived from
        ``KANI_HOME`` and the pinned version.

    Raises
    ------
    ValueError
        If the pin cannot be read or contains no version.
    """
    configured = os.environ.get("KANI_INSTALLATION", "")
    if configured:
        return configured
    version_file = os.environ.get("KANI_VERSION_FILE", DEFAULT_KANI_VERSION_FILE)
    try:
        version = Path(version_file).read_text(encoding="utf-8")
    except OSError as error:
        raise ValueError(
            f"cannot read Kani version file {version_file}: {error.strerror or error}"
        ) from error
    version = "".join(version.split())
    if not version:
        raise ValueError(f"Kani version file {version_file} is empty")
    return str(Path(_setting("KANI_HOME")) / f"kani-{version}")


@dataclass(frozen=True)
class _ArgumentSources:
    """Resolvers for the inputs one prover command needs.

    Attributes
    ----------
    setting : Callable[[str], str]
        Resolves a required non-empty setting by name.
    proof_arguments : Callable[[], list[str]]
        Resolves the Verus proof arguments, including the default proof file.
    kani_installation : Callable[[], str]
        Resolves the Kani release root from the configured pin.
    version_file : str
        Path to the Kani version pin passed to the install and check
        commands.
    """

    setting: Callable[[str], str]
    proof_arguments: Callable[[], list[str]]
    kani_installation: Callable[[], str]
    version_file: str


def _install_kani_arguments(sources: _ArgumentSources) -> list[str]:
    """Return the arguments that install the pinned Kani release."""
    return [
        "kani",
        "install",
        "--no-compile",
        "--version-file",
        sources.version_file,
        "--install-dir",
        sources.setting("KANI_BIN_DIR"),
        "--cache-dir",
        sources.setting("KANI_CACHE_DIR"),
    ]


def _kani_check_arguments(sources: _ArgumentSources) -> list[str]:
    """Return the arguments that verify the pinned Kani release."""
    return [
        "kani",
        "check-version",
        "--version-file",
        sources.version_file,
        "--kani-command",
        shlex.join([str(Path(sources.setting("KANI_BIN_DIR")) / "cargo-kani")]),
    ]


def _kani_exec_arguments(sources: _ArgumentSources, selection: str) -> list[str]:
    """Return the Kani execution arguments for one harness selection."""
    return [
        "kani",
        "exec",
        "--installation",
        sources.kani_installation(),
        "--cwd",
        sources.setting("KANI_PROOF_DIR"),
        selection,
    ]


def _install_verus_arguments(sources: _ArgumentSources) -> list[str]:
    """Return the arguments that install the pinned Verus release."""
    return [
        "verus",
        "install",
        "--version-file",
        sources.setting("VERUS_VERSION_FILE"),
        "--checksum-file",
        sources.setting("VERUS_CHECKSUM_FILE"),
    ]


def _install_z3_arguments(sources: _ArgumentSources) -> list[str]:
    """Return the arguments that install the pinned Z3 solver."""
    return ["z3", "install", "--install-dir", sources.setting("Z3_INSTALL_DIR")]


def _verus_run_arguments(sources: _ArgumentSources) -> list[str]:
    """Return the arguments that run the configured Verus proofs."""
    return ["verus", "run", *sources.proof_arguments()]


_KANI_SELECTIONS = {
    "kani": "--extra-arg=--harness=verify_exact_projection_four_entries",
    "kani-full": "--extra-arg=--output-format=regular",
}

_ARGUMENT_BUILDERS: dict[str, Callable[[_ArgumentSources], list[str]]] = {
    "install-kani": _install_kani_arguments,
    "kani-check": _kani_check_arguments,
    "install-verus": _install_verus_arguments,
    "install-z3": _install_z3_arguments,
    "verus": _verus_run_arguments,
}


def _arguments(action: str, sources: _ArgumentSources) -> list[str]:
    """Return the package arguments for one supported prover action.

    The builder is a query over its injected sources: it reads no environment
    variable itself, so callers can supply the settings and the fallible Kani
    installation resolution explicitly.

    Parameters
    ----------
    action : str
        Make target name requesting the prover command.
    sources : _ArgumentSources
        Resolvers for the settings, proof arguments, and Kani installation
        that the selected command needs.

    Returns
    -------
    list[str]
        Arguments to pass to the configured prover-tools command.

    Raises
    ------
    ValueError
        If ``action`` is not a recognized prover action, a required setting is
        missing, or the Kani pin cannot be read.
    """
    selection = _KANI_SELECTIONS.get(action)
    if selection is not None:
        return _kani_exec_arguments(sources, selection)
    builder = _ARGUMENT_BUILDERS.get(action)
    if builder is None:
        raise ValueError("unknown prover action")
    return builder(sources)


def main() -> int:
    """Run the requested package command without evaluating shell fragments."""
    try:
        if len(sys.argv) != 2:
            raise ValueError("expected one prover action")
        action = sys.argv[1]
        command = _split(_setting("PROVER_TOOLS"), "PROVER_TOOLS")
        if not command:
            raise ValueError("PROVER_TOOLS must contain a command")
        arguments = _arguments(
            action,
            _ArgumentSources(
                setting=_setting,
                proof_arguments=_environment_proof_arguments,
                kani_installation=_kani_installation,
                version_file=os.environ.get(
                    "KANI_VERSION_FILE", DEFAULT_KANI_VERSION_FILE
                ),
            ),
        )
        environment = os.environ.copy()
        # Standalone proof packages do not inherit the root workspace profile.
        environment["CARGO_INCREMENTAL"] = "0"
        if action == "verus":
            environment["VERUS_Z3_PATH"] = str(
                Path(_setting("Z3_INSTALL_DIR")).resolve() / "z3"
            )
        result = subprocess.run([*command, *arguments], env=environment, check=False)
        return result.returncode if result.returncode >= 0 else 128 - result.returncode
    except (OSError, ValueError) as error:
        print(f"prover-tools: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
