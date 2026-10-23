"""Compiler probes: prove that the real compiler rejects unsafe code.

A scratch copy of a crate gets deliberate unsafe constructs appended to each
target; the compiler must reject every one with an unsafe-lint diagnostic. A
control run with the policy stripped must accept the same construct, so a
probe that cannot fail is detected.
"""

from __future__ import annotations

import contextlib
import dataclasses
import os
import re
import shutil
import subprocess
import typing as typ
from pathlib import Path

from .files import read_text, rust_sources
from .inventory import (
    INVENTORY,
    PROBE_DIAGNOSTICS,
    PROBED_CRATES,
    PROBES,
    REJECTION_MARKERS,
    Crate,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Callable, Iterable, Iterator


CompileResult = tuple[int, str]


@dataclasses.dataclass(frozen=True, slots=True)
class ProbeSite:
    """One crate prepared in a scratch workspace and the targets to probe.

    Attributes
    ----------
    crate
        The inventoried crate.
    crate_dir
        The crate's directory inside the scratch workspace.
    targets
        The source files to probe: the library root, a synthesized
        `tests/future_target.rs`, and every existing integration test.
    """

    crate: Crate
    crate_dir: Path
    targets: tuple[Path, ...]


_WORKSPACE_LINT_HEADER = re.compile(r"^\[workspace\.lints(?:\.[^\]]+)?\]\s*$")


def workspace_lint_tables(manifest_text: str) -> str:
    """Return the `[workspace.lints*]` tables of a manifest, verbatim.

    The scratch workspace must carry the same shared lint policy the real
    one does, so a member that inherits it is probed under it.
    """
    kept: list[str] = []
    inside = False
    for line in manifest_text.splitlines():
        if line.startswith("["):
            inside = bool(_WORKSPACE_LINT_HEADER.match(line))
        if inside:
            kept.append(line)
    return "\n".join(kept) + ("\n" if kept else "")


def copy_probe_workspace(root: Path, crate: Crate, scratch: Path) -> ProbeSite:
    """Copy a crate, and what it needs, to a scratch workspace.

    The copy keeps the crate at the same repository-relative path so relative
    `../../wit` paths still resolve, and adds a synthesized test target so
    Cargo's automatic test discovery is probed too.
    """
    if scratch.exists():
        shutil.rmtree(scratch)
    destination = scratch / crate.path
    shutil.copytree(
        root / crate.path,
        destination,
        ignore=shutil.ignore_patterns("target"),
    )
    if (root / "wit").is_dir():
        shutil.copytree(root / "wit", scratch / "wit")
    if not crate.standalone:
        _write_virtual_workspace(root, scratch, crate)
    future = destination / "tests" / "future_target.rs"
    future.parent.mkdir(exist_ok=True)
    future.write_text(
        "//! Probe automatic Cargo target lint inheritance.\n", encoding="utf-8"
    )
    targets = [destination / "src/lib.rs", future]
    targets += sorted((destination / "tests").glob("*.rs"))
    return ProbeSite(crate, destination, tuple(dict.fromkeys(targets)))


def _write_virtual_workspace(root: Path, scratch: Path, crate: Crate) -> None:
    """Write a minimal workspace, with the shared lint tables, around a member."""
    lints = workspace_lint_tables(read_text(root / "Cargo.toml"))
    (scratch / "Cargo.toml").write_text(
        f'[workspace]\nmembers = ["{crate.path}"]\nresolver = "3"\n\n{lints}',
        encoding="utf-8",
    )
    lock = root / "Cargo.lock"
    if lock.exists():
        shutil.copy(lock, scratch / "Cargo.lock")


@contextlib.contextmanager
def _probe_appended(path: Path, probe: str) -> Iterator[None]:
    """Append a probe to a source file, restoring the original afterwards."""
    original = path.read_text(encoding="utf-8")
    path.write_text(original + "\n" + probe + "\n", encoding="utf-8")
    try:
        yield
    finally:
        path.write_text(original, encoding="utf-8")


def _rejected(code: int, output: str) -> bool:
    """Report a compile failure that names the unsafe lint."""
    return code != 0 and any(marker in output for marker in REJECTION_MARKERS)


def strip_policy(site: ProbeSite) -> None:
    """Remove every unsafe prohibition from a scratch copy, for the control run."""
    manifest = site.crate_dir / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    text = re.sub(r'unsafe_code\s*=\s*"(?:forbid|deny)"', 'unsafe_code = "allow"', text)
    # A member that inherits the workspace table cannot override it locally,
    # so give it a table of its own that allows unsafe code.
    text = re.sub(
        r"\[lints\]\s*\nworkspace\s*=\s*true",
        '[lints.rust]\nunsafe_code = "allow"',
        text,
    )
    manifest.write_text(text, encoding="utf-8")
    for source in rust_sources(site.crate_dir):
        original = source.read_text(encoding="utf-8")
        stripped = re.sub(r"#!\[\s*forbid\(\s*unsafe_code\s*\)\s*\]", "", original)
        if stripped != original:
            source.write_text(stripped, encoding="utf-8")


def run_probes(
    site: ProbeSite,
    compile_site: Callable[[ProbeSite], CompileResult],
    logs: Path,
    *,
    control: bool = True,
) -> list[str]:
    """Probe every target of a site with every unsafe form together.

    Parameters
    ----------
    site
        The prepared scratch copy.
    compile_site
        Seam that compiles the copy and returns `(exit code, output)`.
    logs
        Directory for per-probe logs.
    control
        Also prove sensitivity: with the policy stripped, a probe must compile.

    Returns
    -------
    list of str
        One message per failure; empty when every probe was rejected.
    """
    logs.mkdir(parents=True, exist_ok=True)
    slug = site.crate.path.replace("/", "-")
    code, output = compile_site(site)
    (logs / f"{slug}-baseline.log").write_text(output, encoding="utf-8")
    if code != 0:
        return [f"{site.crate.path}: baseline does not compile (exit {code})"]
    problems = _probe_every_target(site, compile_site, logs, slug)
    if control:
        problems += _control(site, compile_site, logs, slug)
    return problems


def _probe_every_target(
    site: ProbeSite,
    compile_site: Callable[[ProbeSite], CompileResult],
    logs: Path,
    slug: str,
) -> list[str]:
    """Require rejection of every unsafe form in every target.

    All forms go into a target together, so one compile per target suffices:
    rustc reports each rejected construct separately, and the phrase for each
    must appear.
    """
    problems = []
    for target in site.targets:
        with _probe_appended(target, "\n".join(PROBES)):
            code, output = compile_site(site)
        (logs / f"{slug}-{target.stem}-unsafe.log").write_text(output, encoding="utf-8")
        problems += _rejection_problems(site, target, code, output)
    return problems


def _rejection_problems(
    site: ProbeSite, target: Path, code: int, output: str
) -> list[str]:
    """Return why a probed target was not rejected for the right reasons."""
    where = f"{site.crate.path}: {target.name}"
    if code == 0:
        return [f"{where} accepted the unsafe probes"]
    if not _rejected(code, output):
        return [f"{where} failed without an unsafe-lint diagnostic"]
    return [
        f"{where} did not report {phrase!r}"
        for phrase in PROBE_DIAGNOSTICS.values()
        if phrase not in output
    ]


def _control(
    site: ProbeSite,
    compile_site: Callable[[ProbeSite], CompileResult],
    logs: Path,
    slug: str,
) -> list[str]:
    """Show that the probes only fail because of the policy under test."""
    strip_policy(site)
    target = site.targets[0]
    with _probe_appended(target, PROBES[0]):
        code, output = compile_site(site)
    (logs / f"{slug}-control.log").write_text(output, encoding="utf-8")
    if code != 0:
        return [f"{site.crate.path}: probe still fails with the policy stripped"]
    return []


#: Seconds one `cargo check` may take. A shared build pool can queue a compile for
#: longer than the default, so the limit is overridable.
COMPILE_TIMEOUT_ENV = "UNSAFE_BOUNDARY_COMPILE_TIMEOUT"


def cargo_compile(site: ProbeSite, target_dir: Path) -> CompileResult:
    """Compile a probe site with Cargo, returning the exit code and output."""
    environment = {**os.environ, "CARGO_TARGET_DIR": str(target_dir)}
    cargo = shutil.which("cargo") or "cargo"
    result = subprocess.run(  # noqa: S603 - fixed argument list, no shell
        [
            cargo,
            "check",
            "--all-targets",
            "--manifest-path",
            str(site.crate_dir / "Cargo.toml"),
        ],
        capture_output=True,
        check=False,
        env=environment,
        text=True,
        timeout=float(os.environ.get(COMPILE_TIMEOUT_ENV, "3600")),
    )
    return result.returncode, result.stdout + result.stderr


def crate_named(path: str, inventory: Iterable[Crate] = INVENTORY) -> Crate:
    """Return the inventoried crate at `path`."""
    for crate in inventory:
        if crate.path == path:
            return crate
    message = f"{path} is not an inventoried crate"
    raise ValueError(message)


def probe_repository(
    root: Path, scratch: Path, logs: Path, target_dir: Path
) -> list[str]:
    """Probe every crate in `PROBED_CRATES` against the real compiler."""
    problems: list[str] = []
    for path in PROBED_CRATES:
        site = copy_probe_workspace(
            root, crate_named(path), scratch / path.replace("/", "-")
        )
        problems += run_probes(site, lambda s: cargo_compile(s, target_dir), logs)
    return problems
