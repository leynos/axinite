"""Contracts for serial scheduling of tests that share trybuild preparation."""

import subprocess
from pathlib import Path

import pytest
from _workflow_policy import REPOSITORY_ROOT
from contract_sources import read_source
from nextest_boundary_support import (
    Fixture,
    _require_the_toolchain,
    _run,
    fixture_crate,
)
from timeout_budgets import NEXTEST_CONFIG, compile_contract_binaries

COMPILE_CONTRACT_GROUP = "compile-contracts"
OVERLAP_MARKER_ENV = "AXINITE_NEXTEST_OVERLAP_MARKER"


def _run_show_groups(
    fixture_crate: Fixture, profile: str, config: Path = NEXTEST_CONFIG
) -> subprocess.CompletedProcess[str]:
    """Ask nextest for effective group membership under one profile."""
    return _run(
        (
            "cargo",
            "nextest",
            "show-config",
            "test-groups",
            "--config-file",
            str(config),
            "--profile",
            profile,
            "--show-default",
            "--no-pager",
            "--color",
            "never",
        ),
        fixture_crate.crate,
        fixture_crate.target,
    )


def _group_section(output: str, group: str) -> str:
    """Return one nextest group section, or an empty string if absent."""
    lines = output.splitlines()
    start = next(
        (
            index
            for index, line in enumerate(lines)
            if line == f"group: {group}" or line.startswith(f"group: {group} (")
        ),
        None,
    )
    if start is None:
        return ""
    end = next(
        (
            index
            for index in range(start + 1, len(lines))
            if lines[index].startswith("group: ")
        ),
        len(lines),
    )
    return "\n".join(lines[start:end])


def _ci_override(config_text: str) -> tuple[str, str, str]:
    """Split the file at its one CI compile-contract override."""
    marker = "[[profile.ci.overrides]]"
    assert config_text.count(marker) == 1, (
        "the runner contract expects one compile-contract override in ci"
    )
    before, block = config_text.split(marker, maxsplit=1)
    return before, marker, block


def _without_ci_group_assignment(config_text: str) -> str:
    """Remove the CI assignment to expose inherited default-profile policy."""
    before, marker, block = _ci_override(config_text)
    assignment = f"test-group = '{COMPILE_CONTRACT_GROUP}'\n"
    assert block.count(assignment) == 1, (
        "the CI override must assign the group before inheritance is tested"
    )
    return before + marker + block.replace(assignment, "", 1)


def _with_ci_group_override(config_text: str, group: str) -> str:
    """Give CI a distinct group to verify selected-profile precedence."""
    before, marker, block = _ci_override(config_text)
    assignment = f"test-group = '{COMPILE_CONTRACT_GROUP}'\n"
    assert block.count(assignment) == 1, (
        "the CI override must assign the shared group before precedence is tested"
    )
    changed = block.replace(assignment, f"test-group = '{group}'\n", 1)
    probe = f"\n[test-groups.{group}]\nmax-threads = 1\n"
    return before + marker + changed + probe


def _without_any_compile_contract_group(config_text: str) -> str:
    """Remove each profile assignment for the negative scheduling control."""
    assignment = f"test-group = '{COMPILE_CONTRACT_GROUP}'\n"
    assert config_text.count(assignment) == 2, (
        "the mutated configuration must remove the default and ci assignments"
    )
    return config_text.replace(assignment, "")


def _run_overlap_fixture(
    fixture_crate: Fixture, config: Path, marker: Path
) -> subprocess.CompletedProcess[str]:
    """Run both fixture binaries with four nextest test slots."""
    return _run(
        (
            "cargo",
            "nextest",
            "run",
            "--config-file",
            str(config),
            "--profile",
            "ci",
            "--jobs",
            "4",
            "-E",
            "binary(trybuild) | binary(schema_helpers_ui)",
        ),
        fixture_crate.crate,
        fixture_crate.target,
        {OVERLAP_MARKER_ENV: str(marker)},
    )


@pytest.mark.parametrize("profile", ["default", "ci", "coverage"], ids=str)
def test_nextest_assigns_compile_contracts_to_the_serial_group(
    fixture_crate: Fixture, profile: str
) -> None:
    """The pinned runner applies serial scheduling to selected targets."""
    done = _run_show_groups(fixture_crate, profile)
    assert done.returncode == 0, (
        f"nextest could not resolve groups for {profile}:\n{done.stderr}"
    )
    group = _group_section(done.stdout, COMPILE_CONTRACT_GROUP)
    assert "max threads = 1" in group, (
        f"nextest does not report a one-thread compile-contract group under "
        f"{profile}:\n{done.stdout}"
    )
    discovered = compile_contract_binaries(REPOSITORY_ROOT / "tests")
    if profile == "default":
        expected = discovered - {"trybuild"}
    elif profile == "coverage":
        expected = frozenset()
    else:
        expected = discovered
    missing = {binary for binary in expected if f"::{binary}:" not in group}
    assert not missing, (
        f"nextest leaves compile-contract binaries outside the serial group "
        f"under {profile}: {sorted(missing)}\n{done.stdout}"
    )
    ordinary = _group_section(done.stdout, "@global")
    assert "::slow:" in ordinary and "::slow:" not in group, (
        "ordinary tests must remain in nextest's parallel global group, "
        f"outside compile-contract serialization:\n{done.stdout}"
    )


def test_coverage_profile_excludes_compile_contracts_but_keeps_ordinary_tests(
    fixture_crate: Fixture,
) -> None:
    """Coverage lists runtime tests while the required ci job runs fixtures."""
    done = _run(
        (
            "cargo",
            "nextest",
            "list",
            "--config-file",
            str(NEXTEST_CONFIG),
            "--profile",
            "coverage",
            "--color",
            "never",
        ),
        fixture_crate.crate,
        fixture_crate.target,
    )
    assert done.returncode == 0, (
        f"nextest could not list the coverage profile:\n{done.stderr}"
    )
    discovered = compile_contract_binaries(REPOSITORY_ROOT / "tests")
    listed = done.stdout
    leaked = {binary for binary in discovered if f"{binary}::" in listed}
    assert not leaked, (
        "coverage profile selects compile-contract fixtures for instrumented "
        f"compilation: {sorted(leaked)}\n{listed}"
    )
    assert "::slow " in listed, (
        "coverage profile must retain ordinary tests while excluding only "
        f"compile contracts:\n{listed}"
    )


def test_ci_inherits_default_compile_contract_group(
    fixture_crate: Fixture, tmp_path: Path
) -> None:
    """The default override still governs CI when CI leaves a field unset."""
    inherited = tmp_path / "ci-inherits-group.toml"
    original = read_source(NEXTEST_CONFIG)
    inherited.write_text(_without_ci_group_assignment(original), encoding="utf-8")
    done = _run_show_groups(fixture_crate, "ci", inherited)
    assert done.returncode == 0, f"nextest refused inherited config:\n{done.stderr}"
    group = _group_section(done.stdout, COMPILE_CONTRACT_GROUP)
    discovered = compile_contract_binaries(REPOSITORY_ROOT / "tests")
    missing = {binary for binary in discovered if f"::{binary}:" not in group}
    assert not missing, (
        "CI must inherit the default profile's group assignment when its own "
        f"override omits it; missing {sorted(missing)}:\n{done.stdout}"
    )


def test_ci_compile_contract_group_override_takes_precedence(
    fixture_crate: Fixture, tmp_path: Path
) -> None:
    """An explicit CI group assignment wins over the inherited default."""
    probe_group = "ci-compile-contract-probe"
    overridden = tmp_path / "ci-overrides-group.toml"
    original = read_source(NEXTEST_CONFIG)
    overridden.write_text(
        _with_ci_group_override(original, probe_group), encoding="utf-8"
    )
    done = _run_show_groups(fixture_crate, "ci", overridden)
    assert done.returncode == 0, f"nextest refused override config:\n{done.stderr}"
    selected = _group_section(done.stdout, probe_group)
    inherited = _group_section(done.stdout, COMPILE_CONTRACT_GROUP)
    discovered = compile_contract_binaries(REPOSITORY_ROOT / "tests")
    missing = {binary for binary in discovered if f"::{binary}:" not in selected}
    inherited_members = {
        binary for binary in discovered if f"::{binary}:" in inherited
    }
    assert not missing and not inherited_members, (
        "CI's explicit group must replace the inherited default assignment; "
        f"missing={sorted(missing)}, inherited={sorted(inherited_members)}"
    )


def test_serial_group_prevents_cross_process_overlap(
    fixture_crate: Fixture, tmp_path: Path
) -> None:
    """The real policy passes and removing its assignments triggers a race."""
    real_marker = tmp_path / "real-overlap.marker"
    real = _run_overlap_fixture(fixture_crate, NEXTEST_CONFIG, real_marker)
    assert real.returncode == 0, (
        "compile-contract fixture processes overlapped under the real "
        f"configuration:\n{real.stdout}\n{real.stderr}"
    )
    assert not real_marker.exists(), "the real run left its marker behind"

    parallel_config = tmp_path / "parallel-compile-contracts.toml"
    original = read_source(NEXTEST_CONFIG)
    parallel_config.write_text(
        _without_any_compile_contract_group(original), encoding="utf-8"
    )
    parallel_marker = tmp_path / "parallel-overlap.marker"
    parallel = _run_overlap_fixture(fixture_crate, parallel_config, parallel_marker)
    output = parallel.stdout + parallel.stderr
    assert parallel.returncode != 0, (
        "the overlap detector passed after both serial assignments were "
        f"removed:\n{output}"
    )
    assert "compile-contract test processes overlapped" in output, (
        "the mutated run failed without the overlap diagnostic, so it does "
        f"not prove the scheduler contract:\n{output}"
    )
