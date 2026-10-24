"""Contract that the pull-request lane measures what the publisher baselines.

The shared CV-005 library compares the two sides' coverage selection (features,
default features, nextest, format, output path and ratchet), which is declared
in `.github/cv005.toml`. It does not compare the rest of what makes two
coverage numbers comparable: the Rust toolchain they are measured with, the
nextest profile that decides which tests run, and the watchdog around the
build. A ratchet that compares a report built with a different toolchain or
profile than its baseline reports that difference as a change in coverage, so
those inputs are held equal here.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _workflow_files import jobs_of

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from _estate import Estate

LANE: typ.Final[tuple[str, str, str]] = (
    "codescene-coverage.yml",
    "coverage-check",
    "Generate coverage",
)
PUBLISHER: typ.Final[tuple[str, str, str]] = (
    "coverage.yml",
    "coverage-libsql",
    "Generate coverage and the ratchet baseline",
)

RUST_TOOLCHAIN_ACTION: typ.Final[str] = "dtolnay/rust-toolchain@"
SETUP_RUST_ACTION: typ.Final[str] = "setup-rust@"


def _job(estate: Estate, workflow: str, job_id: str) -> dict[str, object]:
    """Return one job's body, failing loudly if it is renamed away."""
    for job in jobs_of(workflow, estate[workflow]):
        if job.job_id == job_id:
            return job.body
    message = f"{workflow} must declare job {job_id!r}"
    raise AssertionError(message)


def _step(job: dict[str, object], name: str) -> dict[str, object]:
    """Return the one step with a name."""
    steps = job.get("steps")
    assert isinstance(steps, list), "the job must declare steps"
    matches = [s for s in steps if isinstance(s, dict) and s.get("name") == name]
    assert len(matches) == 1, f"expected one {name!r} step, found {len(matches)}"
    return matches[0]


def _uses_step(job: dict[str, object], fragment: str) -> dict[str, object]:
    """Return the one step whose `uses` contains a fragment."""
    steps = job.get("steps")
    assert isinstance(steps, list), "the job must declare steps"
    matches = [
        s for s in steps if isinstance(s, dict) and fragment in str(s.get("uses", ""))
    ]
    assert len(matches) == 1, f"expected one {fragment!r} step, found {len(matches)}"
    return matches[0]


def _with(step: dict[str, object]) -> dict[str, object]:
    """Return a step's `with` mapping."""
    inputs = step.get("with")
    assert isinstance(inputs, dict), f"{step.get('name') or step.get('uses')} needs with"
    return inputs


def _sides(estate: Estate) -> tuple[dict[str, object], dict[str, object]]:
    """Return the lane's and the publisher's jobs."""
    return _job(estate, LANE[0], LANE[1]), _job(estate, PUBLISHER[0], PUBLISHER[1])


def test_both_sides_measure_with_the_same_toolchain(estate: Estate) -> None:
    """Both sides take one toolchain, from `rust-toolchain.toml`, and agree.

    The build standard pins the toolchain in `rust-toolchain.toml`, so neither
    side installs one by hand or names a `toolchain` input to `setup-rust`;
    either would let the two measurements drift apart. The `setup-rust`
    revision is held equal.
    """
    lane, publisher = _sides(estate)
    for side, job in (("lane", lane), ("publisher", publisher)):
        steps = job.get("steps")
        assert isinstance(steps, list), "the job must declare steps"
        installers = [
            s
            for s in steps
            if isinstance(s, dict) and RUST_TOOLCHAIN_ACTION in str(s.get("uses", ""))
        ]
        assert not installers, (
            f"the {side} installs a toolchain by hand; it must come from "
            "rust-toolchain.toml"
        )
        assert "toolchain" not in _with(_uses_step(job, SETUP_RUST_ACTION)), (
            f"the {side} names a setup-rust toolchain; it must follow "
            "rust-toolchain.toml"
        )
    lane_step = _uses_step(lane, SETUP_RUST_ACTION)
    publisher_step = _uses_step(publisher, SETUP_RUST_ACTION)
    assert lane_step.get("uses") == publisher_step.get("uses"), (
        "the lane and the publisher use different setup-rust revisions: "
        f"{lane_step.get('uses')!r} against {publisher_step.get('uses')!r}"
    )


@pytest.mark.parametrize(
    "where",
    ["env", "cargo-wait-timeout", "cache-provider", "uses"],
)
def test_both_sides_run_the_same_generator_configuration(
    estate: Estate, where: str
) -> None:
    """The generator step's profile, watchdog, cache owner and pin agree.

    `env` carries `NEXTEST_PROFILE`, which decides which tests the report is
    built from; `cargo-wait-timeout` bounds the build; `cache-provider` says
    who owns compiler output; and `uses` is the action revision itself, so the
    two sides cannot measure with different generators.
    """
    lane_job, publisher_job = _sides(estate)
    lane = _step(lane_job, LANE[2])
    publisher = _step(publisher_job, PUBLISHER[2])
    if where in {"env", "uses"}:
        lane_value, publisher_value = lane.get(where), publisher.get(where)
    else:
        lane_value = _with(lane).get(where)
        publisher_value = _with(publisher).get(where)
    assert lane_value is not None, f"the lane's generator must set {where}"
    assert lane_value == publisher_value, (
        f"generator {where} differs: {lane_value!r} in the lane, "
        f"{publisher_value!r} in the publisher"
    )
