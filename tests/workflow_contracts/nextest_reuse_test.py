"""Prove prepared metadata prevents Cargo rebuilds in live Nextest probes."""

from __future__ import annotations

import json
from pathlib import Path

import pytest
import nextest_boundary_support as support
from nextest_boundary_support import Fixture
from nextest_fixture import (
    EXPECTED_FIXTURE_BINARIES,
    FixturePreparationError,
    _nextest_probe_arguments,
    _validate_binaries_metadata,
)
from nextest_probe_support import (
    assert_metadata_is_rejected,
    assert_probe_result,
    assert_single_probe_invocation,
    create_cargo_proxy,
    execute_probe_case,
    select_probe_case,
)
from nextest_test_support import (
    SyntheticTiming,
    binary_inventory,
    binary_records,
    fixture_with_saved_metadata,
    synthetic_process_result,
)


@pytest.mark.parametrize(
    "command,subcommand",
    [
        (("cargo", "nextest", "run", "-E", "all"), "run"),
        (("cargo", "nextest", "list", "--color", "never"), "list"),
        (
            ("cargo", "nextest", "show-config", "test-groups", "--profile", "ci"),
            "test-groups",
        ),
    ],
    ids=["run", "list", "show-config-test-groups"],
)
def test_supported_probe_forms_reuse_both_saved_metadata(
    tmp_path: Path, command: tuple[str, ...], subcommand: str
) -> None:
    """Every supported probe receives both complete preparation inputs."""
    fixture = fixture_with_saved_metadata(tmp_path)
    prepared = _nextest_probe_arguments(command, fixture)
    if subcommand == "test-groups":
        assert prepared[2:4] == ("show-config", "test-groups"), (
            "show-config metadata flags must follow the subcommand pair"
        )
    else:
        assert prepared[2] == subcommand, "the supported Nextest subcommand must be retained"
    assert prepared.count("--cargo-metadata") == 1, (
        "a probe must receive Cargo metadata exactly once"
    )
    assert prepared.count("--binaries-metadata") == 1, (
        "a probe must receive binary metadata exactly once"
    )
    cargo_index = prepared.index("--cargo-metadata")
    binaries_index = prepared.index("--binaries-metadata")
    assert Path(prepared[cargo_index + 1]) == fixture.cargo_metadata, (
        "a probe must reuse the prepared Cargo metadata path"
    )
    assert Path(prepared[binaries_index + 1]) == fixture.binaries_metadata, (
        "a probe must reuse the prepared binary inventory path"
    )


def test_fixture_preparation_receipt_records_two_ordered_phases(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    tmp_path_factory: pytest.TempPathFactory,
) -> None:
    """Tool checks precede prep and successful phases leave one receipt."""
    fixture_source = tmp_path / "fixture-source"
    fixture_source.mkdir()
    events: list[str] = []
    monkeypatch.setattr(support, "FIXTURE_CRATE", fixture_source)
    monkeypatch.setattr(support, "REPOSITORY_ROOT", tmp_path)
    monkeypatch.setattr(
        support, "_require_the_toolchain", lambda: events.append("toolcheck")
    )

    def prepare(root: Path, crate: Path, target: Path) -> Fixture:
        events.append("prepare")
        assert crate.is_dir() and crate.parent == root, (
            "the fixture source must be copied beneath its unique run root"
        )
        records = (
            synthetic_process_result(
                "cargo metadata preparation",
                ("cargo", "metadata"),
                timing=SyntheticTiming(elapsed_seconds=2),
            ),
            synthetic_process_result(
                "Nextest binary inventory preparation",
                ("cargo", "nextest", "list"),
                timing=SyntheticTiming(elapsed_seconds=3),
            ),
        )
        return Fixture(
            crate=crate,
            target=target,
            cargo_metadata=root / "cargo.json",
            binaries_metadata=root / "binaries.json",
            preparation_records=records,
        )

    monkeypatch.setattr(support, "prepare_fixture", prepare)
    fixture = support.prepare_fixture_crate(tmp_path_factory)
    assert events == ["toolcheck", "prepare"], (
        "tool availability must be checked before fixture preparation"
    )
    receipt_dir = fixture.target / "preparation-receipts"
    receipts = list(receipt_dir.glob("*.jsonl"))
    assert len(receipts) == 1, "one fixture preparation must leave one receipt"
    receipt = receipts[0]
    rows = [
        json.loads(line)
        for line in receipt.read_text(encoding="utf-8").splitlines()
    ]
    assert [row["phase"] for row in rows] == [
        "cargo metadata preparation",
        "Nextest binary inventory preparation",
    ], "the receipt must retain both preparation phases in order"
    assert [(row["elapsed_seconds"], row["status"]) for row in rows] == [
        (2, 0),
        (3, 0),
    ], "the receipt must retain successful elapsed times and statuses"
    assert [row["command"] for row in rows] == [
        ["cargo", "metadata"],
        ["cargo", "nextest", "list"],
    ], "the receipt must identify the exact preparation commands"
    assert rows[0]["copy"] == rows[1]["copy"] == str(fixture.crate.parent), (
        "both preparation records must point to the same fixture copy"
    )


@pytest.mark.parametrize(
    "probe",
    ["show-config", "list", "run", "malformed-metadata"],
    ids=["groups", "list-tests", "run-one-contract", "reject-invalid-json"],
)
def test_live_nextest_probes_do_not_spawn_a_second_cargo_command(
    fixture_crate: Fixture, tmp_path: Path, probe: str
) -> None:
    """Real probes reuse inputs or reject a bad copy before nested Cargo."""
    proxy = create_cargo_proxy(tmp_path)
    assert_metadata_is_rejected(proxy, fixture_crate.crate)
    case = select_probe_case(probe, fixture_crate, tmp_path)
    result = execute_probe_case(case, fixture_crate, proxy)
    assert_probe_result(case, result)
    assert_single_probe_invocation(proxy, case)


@pytest.mark.parametrize(
    "metadata,diagnostic",
    [
        (
            json.dumps(
                {
                    "rust-build-meta": {
                        "schema_helpers_ui": "trybuild slow"
                    },
                    "rust-binaries": {
                        "schema_helpers_ui": {"binary-name": "other"}
                    },
                }
            ),
            "omitted fixture targets",
        ),
        (
            json.dumps(
                {
                    "rust-build-meta": {},
                    "rust-binaries": list(
                        binary_records(EXPECTED_FIXTURE_BINARIES).values()
                    ),
                }
            ),
            "must contain object fields",
        ),
        (
            json.dumps(binary_inventory()) + json.dumps(binary_inventory()),
            "one valid JSON object",
        ),
        (
            json.dumps(
                {
                    "rust-build-meta": {},
                    "rust-binaries": binary_records(
                        EXPECTED_FIXTURE_BINARIES - {"slow"}
                    ),
                }
            ),
            "omitted fixture targets",
        ),
        (
            json.dumps(
                {
                    "rust-build-meta": {},
                    "rust-binaries": {"fixture-target": "schema_helpers_ui"},
                }
            ),
            "string 'binary-name' field",
        ),
    ],
    ids=["misleading", "wrong-shape", "json-stream", "incomplete", "bad-record"],
)
def test_binary_inventory_requires_structural_complete_summary(
    metadata: str, diagnostic: str
) -> None:
    """Only complete binary-name records count toward the saved inventory."""
    with pytest.raises(FixturePreparationError, match=diagnostic):
        _validate_binaries_metadata(metadata)
