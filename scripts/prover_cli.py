"""Forward Make's prover settings as literal arguments to rust-prover-tools."""

from __future__ import annotations

import os
import shlex
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

DEFAULT_KANI_VERSION_FILE = "tools/kani/VERSION"


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
    value = os.environ.get(name, "")
    if not value:
        raise ValueError(f"{name} must be configured")
    return value


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


def _proof_arguments() -> list[str]:
    """Return Verus proof arguments, inserting the default proof file if needed.

    Returns
    -------
    list[str]
        ``VERUS_FLAGS`` as literal arguments, preceded by the configured proof
        file when the flags do not already select one.
    """
    flags = _split(os.environ.get("VERUS_FLAGS", ""), "VERUS_FLAGS")
    has_proof_file = any(
        flag == "--proof-file" or flag.startswith("--proof-file=") for flag in flags
    )
    return (
        flags
        if has_proof_file
        else ["--proof-file", _setting("VERUS_PROOF_FILE"), *flags]
    )


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


def _arguments(
    action: str,
    *,
    setting: Callable[[str], str],
    proof_arguments: Callable[[], list[str]],
    kani_installation: Callable[[], str],
    version_file: str,
) -> list[str]:
    """Return the package arguments for one supported prover action.

    The builder is a query over its injected dependencies: it reads no
    environment variable itself, so callers can supply the settings and the
    fallible Kani installation resolution explicitly.

    Parameters
    ----------
    action : str
        Make target name requesting the prover command.
    setting : Callable[[str], str]
        Resolves a required non-empty setting by name.
    proof_arguments : Callable[[], list[str]]
        Resolves the Verus proof arguments, including the default proof file.
    kani_installation : Callable[[], str]
        Resolves the Kani release root from the configured pin.
    version_file : str
        Path to the Kani version pin passed to the install and check
        commands.

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
    if action == "install-kani":
        return [
            "kani",
            "install",
            "--no-compile",
            "--version-file",
            version_file,
            "--install-dir",
            setting("KANI_BIN_DIR"),
            "--cache-dir",
            setting("KANI_CACHE_DIR"),
        ]
    if action == "kani-check":
        return [
            "kani",
            "check-version",
            "--version-file",
            version_file,
            "--kani-command",
            shlex.join([str(Path(setting("KANI_BIN_DIR")) / "cargo-kani")]),
        ]
    if action in {"kani", "kani-full"}:
        selection = (
            "--extra-arg=--harness=verify_exact_projection_four_entries"
            if action == "kani"
            else "--extra-arg=--output-format=regular"
        )
        return [
            "kani",
            "exec",
            "--installation",
            kani_installation(),
            "--cwd",
            setting("KANI_PROOF_DIR"),
            selection,
        ]
    if action == "install-verus":
        return [
            "verus",
            "install",
            "--version-file",
            setting("VERUS_VERSION_FILE"),
            "--checksum-file",
            setting("VERUS_CHECKSUM_FILE"),
        ]
    if action == "install-z3":
        return ["z3", "install", "--install-dir", setting("Z3_INSTALL_DIR")]
    if action == "verus":
        return ["verus", "run", *proof_arguments()]
    raise ValueError("unknown prover action")


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
            setting=_setting,
            proof_arguments=_proof_arguments,
            kani_installation=_kani_installation,
            version_file=os.environ.get(
                "KANI_VERSION_FILE", DEFAULT_KANI_VERSION_FILE
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
