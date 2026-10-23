"""Contracts for the compiler half of the unsafe-boundary contract (ADR 015).

`make boundary-contract` copies a crate to a scratch workspace, appends each
deliberate unsafe probe to every target, and requires the compiler to reject
it; a control run with the policy stripped must accept the same probe. These
contracts drive that harness through a recorded compiler seam, so they assert
what the harness compiles, what it requires of each result, and that it puts
every probed file back, without invoking Cargo.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "scripts"))

from unsafe_boundaries import files, inventory, probes  # noqa: E402

REJECTION = "error: use of unsafe code is forbidden by `-F unsafe-code`"
SAFE = "crates/ambient-fs"
BOUNDARY = "tools-src/web-search"


class RecordedCompiler:
    """Stand in for Cargo, deriving each result from the sources it is given.

    A copy that still carries probes and a prohibition fails with one
    diagnostic per probe present; any other copy compiles. Deriving the result
    from the sources, not from call order, keeps the seam honest: a harness
    that skipped a probe, or failed to remove one, changes what the test
    observes.
    """

    def __init__(self) -> None:
        self.calls: list[tuple[int, bool]] = []

    def __call__(self, site: probes.ProbeSite) -> probes.CompileResult:
        """Record one compile and return the result its sources imply."""
        sources = "\n".join(
            path.read_text(encoding="utf-8")
            for path in sorted(files.rust_sources(site.crate_dir))
        )
        manifest = (site.crate_dir / "Cargo.toml").read_text(encoding="utf-8")
        present = [p for p in inventory.PROBES if p in sources]
        policy = (
            'unsafe_code = "forbid"' in manifest or 'unsafe_code = "deny"' in manifest
        )
        self.calls.append((len(present), policy))
        if present and policy:
            diagnostics = "\n".join(
                f"error: {inventory.PROBE_DIAGNOSTICS[p]}" for p in present
            )
            return 1, f"{diagnostics}\n{REJECTION}"
        return 0, "finished checking"


def _site(tmp_path: Path, path: str) -> probes.ProbeSite:
    """Copy a real crate into a scratch workspace and return its probe site."""
    crate = probes.crate_named(path)
    return probes.copy_probe_workspace(REPOSITORY, crate, tmp_path / "scratch")


@pytest.mark.parametrize("path", [SAFE, BOUNDARY])
def test_every_target_is_probed_with_every_unsafe_form(
    tmp_path: Path, path: str
) -> None:
    """The baseline compiles, then each target takes all the forms in one compile."""
    site = _site(tmp_path, path)
    compiler = RecordedCompiler()
    assert probes.run_probes(site, compiler, tmp_path / "logs") == []
    every_form = len(inventory.PROBES)
    probed = [count for count, policy in compiler.calls if policy and count]
    assert probed == [every_form] * len(site.targets), (
        f"every target of {path} must be compiled once with all {every_form} forms"
    )


@pytest.mark.parametrize("path", [SAFE, BOUNDARY])
def test_a_future_test_target_is_probed(tmp_path: Path, path: str) -> None:
    """Cargo discovers new `tests/*.rs` targets, so one is synthesized and probed."""
    site = _site(tmp_path, path)
    assert any(target.name == "future_target.rs" for target in site.targets)
    assert any(target.name == "lib.rs" for target in site.targets)


def test_every_probed_file_is_restored(tmp_path: Path) -> None:
    """A probe never outlives its compile, even for the control run."""
    site = _site(tmp_path, SAFE)
    before = {t: t.read_text(encoding="utf-8") for t in site.targets}
    probes.run_probes(site, RecordedCompiler(), tmp_path / "logs")
    for target, original in before.items():
        text = target.read_text(encoding="utf-8")
        assert not any(probe in text for probe in inventory.PROBES), target
        if "forbid(unsafe_code)" not in original:
            assert text == original, f"{target} was not restored"


def test_each_result_is_archived_under_its_target(tmp_path: Path) -> None:
    """Every compile leaves a log a reviewer can read."""
    site = _site(tmp_path, SAFE)
    logs = tmp_path / "logs"
    probes.run_probes(site, RecordedCompiler(), logs)
    names = {path.name for path in logs.glob("*.log")}
    expected = {
        f"crates-ambient-fs-{target.stem}-unsafe.log" for target in site.targets
    }
    assert expected <= names
    assert {"crates-ambient-fs-baseline.log", "crates-ambient-fs-control.log"} <= names


def test_the_control_run_strips_the_policy(tmp_path: Path) -> None:
    """The final compile has no prohibition, so the probe must be accepted."""
    site = _site(tmp_path, BOUNDARY)
    compiler = RecordedCompiler()
    probes.run_probes(site, compiler, tmp_path / "logs")
    count, policy = compiler.calls[-1]
    assert count == 1, "the control run compiles one probe"
    assert policy is False, "the control run still carried the prohibition"


def test_a_compiler_that_accepts_unsafe_is_reported(tmp_path: Path) -> None:
    """If rustc accepts a probe, the contract fails and names the target."""
    site = _site(tmp_path, SAFE)

    def lenient(_: probes.ProbeSite) -> probes.CompileResult:
        return 0, "finished checking"

    problems = probes.run_probes(site, lenient, tmp_path / "logs")
    assert any("accepted the unsafe probes" in problem for problem in problems)
    assert any("lib.rs" in problem for problem in problems)


def test_an_unrelated_compile_error_is_not_a_rejection(tmp_path: Path) -> None:
    """A probe that fails for another reason proves nothing about the policy."""
    site = _site(tmp_path, SAFE)

    def broken(_: probes.ProbeSite) -> probes.CompileResult:
        return 101, "error[E0425]: cannot find value `x` in this scope"

    problems = probes.run_probes(site, broken, tmp_path / "logs")
    assert any("baseline does not compile" in problem for problem in problems)


def test_a_probe_that_cannot_fail_is_reported(tmp_path: Path) -> None:
    """If the stripped control still fails, the probe is not testing the policy."""
    site = _site(tmp_path, SAFE)

    def always_rejecting(compile_site: probes.ProbeSite) -> probes.CompileResult:
        sources = "\n".join(
            path.read_text(encoding="utf-8")
            for path in files.rust_sources(compile_site.crate_dir)
        )
        if any(probe in sources for probe in inventory.PROBES):
            return 1, REJECTION
        return 0, "finished checking"

    problems = probes.run_probes(site, always_rejecting, tmp_path / "logs")
    assert any("probe still fails with the policy stripped" in p for p in problems)


def test_a_rejection_that_omits_a_form_is_reported(tmp_path: Path) -> None:
    """Every form must be named by the compiler, not just one of them."""
    site = _site(tmp_path, SAFE)
    recorded = RecordedCompiler()
    omitted = inventory.PROBE_DIAGNOSTICS[inventory.PROBES[1]]

    def partial(compile_site: probes.ProbeSite) -> probes.CompileResult:
        code, output = recorded(compile_site)
        return code, output.replace(omitted, "an unrelated message")

    problems = probes.run_probes(site, partial, tmp_path / "logs")
    assert any("did not report" in p and omitted in p for p in problems)


def test_a_probe_that_fails_for_another_reason_is_reported(tmp_path: Path) -> None:
    """A compile error that is not the unsafe lint proves nothing about it."""
    site = _site(tmp_path, SAFE)
    compiles: list[None] = []

    def unrelated(_: probes.ProbeSite) -> probes.CompileResult:
        compiles.append(None)
        if len(compiles) == 1:
            return 0, "finished checking"
        return 101, "error[E0425]: cannot find value `x` in this scope"

    problems = probes.run_probes(site, unrelated, tmp_path / "logs")
    assert any("failed without an unsafe-lint diagnostic" in p for p in problems)


def test_a_missing_baseline_is_reported_before_any_probe(tmp_path: Path) -> None:
    """A copy that does not compile cannot be probed, and says so."""
    site = _site(tmp_path, SAFE)
    seen: list[probes.ProbeSite] = []

    def failing(compile_site: probes.ProbeSite) -> probes.CompileResult:
        seen.append(compile_site)
        return 1, "error: could not compile"

    problems = probes.run_probes(site, failing, tmp_path / "logs")
    assert problems == [f"{SAFE}: baseline does not compile (exit 1)"]
    assert len(seen) == 1


def test_a_boundary_copy_keeps_the_wit_definitions(tmp_path: Path) -> None:
    """`generate!` resolves `../../wit`, so the copy must keep that layout."""
    site = _site(tmp_path, BOUNDARY)
    assert (tmp_path / "scratch" / "wit" / "tool.wit").is_file()
    assert site.crate_dir == tmp_path / "scratch" / BOUNDARY


def test_a_member_copy_carries_the_shared_lint_tables(tmp_path: Path) -> None:
    """A member that inherits workspace lints must be probed under them."""
    manifest = (
        '[package]\nname = "x"\n\n[workspace.lints.rust]\nunsafe_code = "forbid"\n\n'
        '[workspace.lints.clippy]\npedantic = "warn"\n\n[profile.dev]\nopt-level = 1\n'
    )
    tables = probes.workspace_lint_tables(manifest)
    assert "[workspace.lints.rust]" in tables
    assert "[workspace.lints.clippy]" in tables
    assert "profile" not in tables
    assert "[package]" not in tables


def test_stripping_the_policy_also_releases_inherited_lints(tmp_path: Path) -> None:
    """The control must work after the lint-config layer replaces local tables."""
    site = _site(tmp_path, SAFE)
    manifest = site.crate_dir / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8").replace(
        '[lints.rust]\nunsafe_code = "forbid"', "[lints]\nworkspace = true"
    )
    manifest.write_text(text, encoding="utf-8")
    probes.strip_policy(site)
    stripped = manifest.read_text(encoding="utf-8")
    assert 'unsafe_code = "allow"' in stripped
    assert "workspace = true" not in stripped
