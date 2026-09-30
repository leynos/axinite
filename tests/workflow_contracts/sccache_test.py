"""Contracts for the sccache wiring on Ubicloud Rust jobs.

Installing sccache does nothing on its own. Cargo only routes compilation
through it when `RUSTC_WRAPPER` names it, and its GitHub Actions backend only
reaches Ubicloud's store when the runner's cache endpoint is republished and
the v2 cache-service flag the proxy does not serve is cleared. Either omission
is silent: the build succeeds, the job just recompiles everything.

The shared `setup-rust` action now does all of that, selecting the backend from
the runner (ADR 0005 in leynos/shared-actions). These contracts hold every
compiling Ubicloud job to it: one pinned call with sccache on, the
`expect-cache` its placement allows, an empty `rustflags` so the mold flags in
`CARGO_TARGET_*_RUSTFLAGS` survive, and none of the hand-rolled pieces it
replaced, because a caller's wrapper or backend switch overrides the action's
choice without a word.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
from collections import abc

import pytest
from _workflow_files import jobs
from _workflow_policy import Job

ALL_JOBS: tuple[Job, ...] = tuple(jobs())

SETUP_STEP = "Setup Rust"
SETUP_ID = "setup-rust"
REPORT_STEP = "Report sccache statistics"

#: The shared action that owns sccache, pinned to a full commit.
SETUP_RUST = re.compile(
    r"^leynos/shared-actions/\.github/actions/setup-rust@[0-9a-f]{40}$"
)

#: Labels served by GitHub's own pool. A job that can land on one, through a
#: schedule arm or a fork fallback, must accept whatever backend the runner
#: offers there.
GITHUB_HOSTED_LABELS: frozenset[str] = frozenset(
    {"ubuntu-latest", "ubuntu-24.04", "ubuntu-22.04", "windows-latest", "macos-latest"}
)

#: Variables that configured the retired hand-rolled wiring. `setup-rust`
#: sets or selects each of them, and a caller's value wins over its choice.
RETIRED_VARIABLES: tuple[str, ...] = (
    "RUSTC_WRAPPER",
    "SCCACHE_GHA_ENABLED",
    "SCCACHE_DIR",
    "SCCACHE_CACHE_SIZE",
)

#: Steps the retired wiring ran, by name.
RETIRED_STEPS: tuple[str, ...] = (
    "Export the Actions cache endpoint for sccache",
    "Install sccache",
    "Start sccache statistics",
)

#: A script that starts or resets the server, which `setup-rust` does now.
SERVER_COMMAND = re.compile(r"\bsccache\s+--(?:zero-stats|start-server)\b")

#: Actions that installed sccache by hand. Matched on `uses` and `with.tool`,
#: so renaming the step hides nothing.
SCCACHE_INSTALLER = re.compile(
    r"^mozilla-actions/sccache-action@|^taiki-e/install-action@"
)

#: Variables the retired export step republished from the runner. A script or
#: action input that names either is rebuilding the endpoint by hand.
CACHE_ENDPOINT_VARIABLES: tuple[str, ...] = (
    "ACTIONS_CACHE_URL",
    "ACTIONS_RUNTIME_TOKEN",
)


#: Commands that actually invoke rustc, and therefore benefit from a compiler
#: cache. `cargo fmt` is absent on purpose: the formatter gate needs the
#: toolchain but compiles nothing, so wrapping it would add an install for no
#: cache traffic. `docker build` is absent because compilation happens inside
#: the image, where sccache on the host cannot see it. The trailing boundary
#: matters: `make test-workflow-contracts` is a PyYAML parse, not a build.
COMPILING_COMMANDS: tuple[re.Pattern[str], ...] = tuple(
    re.compile(pattern)
    for pattern in (
        # The optional +toolchain segment matters: `cargo +nightly test`
        # invokes rustc just as surely, and a selector that missed it would
        # exempt that job from every assertion below.
        r"\bcargo\s+(?:\+\S+\s+)?(?:build|check|clippy|test|nextest|llvm-cov)\b",
        # Each target is named. A bare `test` with a boundary would match the
        # first half of `test-workflow-contracts`, which is a PyYAML parse and
        # not a build; a `(?!-)` guard on it would instead exempt
        # `test-workspace`, which compiles the whole workspace.
        r"\bmake\s+(?:test-workspace|test-github-tool|test|lint-whitaker)\b(?!-)",
        r"\./scripts/build-wasm-extensions\.sh",
    )
)


def _compiles(job: Job) -> bool:
    """Report whether any of a job's steps invokes the Rust compiler."""
    return any(
        pattern.search(str(step.get("run", "")))
        for step in job.steps
        for pattern in COMPILING_COMMANDS
    )


def _compiling_ubicloud_jobs() -> tuple[Job, ...]:
    """Return the Ubicloud jobs that invoke the Rust compiler."""
    return tuple(job for job in ALL_JOBS if job.uses_ubicloud and _compiles(job))


def _ids(candidates: tuple[Job, ...]) -> list[str]:
    """Return readable parameter identifiers for a job sequence."""
    return [str(job) for job in candidates]


def expected_expect_cache(labels: abc.Iterable[str]) -> str:
    """Return the `expect-cache` value a job's placement calls for.

    Parameters
    ----------
    labels
        Every runner label the job can resolve to.

    Returns
    -------
    str
        ``"any"`` when a GitHub-hosted label is among them, else ``"ubicloud"``.

    Examples
    --------
    >>> expected_expect_cache(["ubuntu-latest", "ubicloud-standard-4"])
    'any'
    >>> expected_expect_cache(["ubicloud-standard-4"])
    'ubicloud'
    """
    return "any" if set(labels) & GITHUB_HOSTED_LABELS else "ubicloud"


def _retired_variables(owner: str, env: object) -> list[str]:
    """Return a finding for each retired variable a job or step `env` sets."""
    if not isinstance(env, dict):
        return []
    return [f"{owner} sets {name}" for name in RETIRED_VARIABLES if name in env]


def _installs_sccache(step: dict[str, object]) -> bool:
    """Report whether a step installs sccache itself, whatever it is named."""
    uses = str(step.get("uses", ""))
    if uses.startswith("mozilla-actions/sccache-action@"):
        return True
    inputs = step.get("with")
    tool = str(inputs.get("tool", "")) if isinstance(inputs, dict) else ""
    return uses.startswith("taiki-e/install-action@") and "sccache" in tool


def _exports_cache_endpoint(step: dict[str, object]) -> bool:
    """Report whether a step republishes the runner's cache endpoint itself."""
    text = f"{step.get('run', '')} {step.get('with', '')}"
    return any(name in text for name in CACHE_ENDPOINT_VARIABLES)


def _retired_in_step(index: int, step: dict[str, object]) -> list[str]:
    """Return every retired piece of sccache wiring in one step."""
    findings = _retired_variables(f"step {index}", step.get("env"))
    if step.get("name") in RETIRED_STEPS:
        findings.append(f"step {index} is the retired {step['name']!r}")
    if _installs_sccache(step):
        findings.append(f"step {index} installs sccache itself")
    if _exports_cache_endpoint(step):
        findings.append(f"step {index} republishes the cache endpoint itself")
    if SERVER_COMMAND.search(str(step.get("run", ""))):
        findings.append(f"step {index} starts or zeroes the sccache server")
    if "ACTIONS_CACHE_SERVICE_V2" in str(step.get("with", "")):
        findings.append(f"step {index} rewrites the cache-service flag itself")
    return findings


def retired_findings(env: object, steps: abc.Sequence[dict[str, object]]) -> list[str]:
    """Return every retired piece of sccache wiring in a job.

    Pure over the parsed data, so the rule runs on fixtures as well as on the
    checked-in workflows.

    Parameters
    ----------
    env
        The job's `env` mapping, or anything else when it declares none.
    steps
        The job's parsed steps.

    Returns
    -------
    list of str
        One description per finding; empty for a job that leaves sccache to
        `setup-rust`.

    Examples
    --------
    >>> retired_findings({}, [{"name": "Setup Rust"}])
    []
    >>> retired_findings({"RUSTC_WRAPPER": "sccache"}, [{"run": "sccache --zero-stats"}])
    ['job sets RUSTC_WRAPPER', 'step 0 starts or zeroes the sccache server']
    """
    findings = _retired_variables("job", env)
    for index, step in enumerate(steps):
        findings.extend(_retired_in_step(index, step))
    return findings


WRAPPED = _compiling_ubicloud_jobs()


def _setup(job: Job) -> dict[str, object]:
    """Return a job's one `Setup Rust` step."""
    calls = [step for step in job.steps if step.get("name") == SETUP_STEP]
    assert len(calls) == 1, f"{job} must run exactly one {SETUP_STEP!r} step"
    return calls[0]


def test_the_wrapped_set_is_not_empty() -> None:
    """Guard against a selector that quietly matches nothing."""
    assert len(WRAPPED) >= 8, (
        "every Ubicloud job that invokes the Rust compiler should be held here"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_setup_rust_owns_the_compiler_cache(job: Job) -> None:
    """One pinned call, sccache on, an id, and inputs that keep the job's own."""
    step = _setup(job)
    assert SETUP_RUST.match(str(step.get("uses", ""))), (
        f"{job} must pin leynos/shared-actions setup-rust to a full commit SHA"
    )
    assert step.get("id") == SETUP_ID, (
        f"{job} must give setup-rust the id {SETUP_ID!r}, or its report "
        "cannot name the backend"
    )
    inputs = step.get("with")
    assert isinstance(inputs, dict), f"{job} must pass setup-rust its inputs"
    assert str(inputs.get("use-sccache", "true")) == "true", (
        f"{job} switches setup-rust's sccache off"
    )
    # setup-rust exports RUSTFLAGS unless told not to, and RUSTFLAGS displaces
    # the CARGO_TARGET_*_RUSTFLAGS that carry this job's mold linker flags.
    assert inputs.get("rustflags") == "", (
        f"{job} must pass an empty rustflags, or the mold flags are displaced"
    )
    assert inputs.get("cache-provider") == "external", (
        f"{job} owns its Cargo registry cache; setup-rust must not own a second"
    )
    # The job installs its own pinned cargo-binstall; setup-rust's default
    # would duplicate or precede it.
    assert inputs.get("install-binstall") == "false", (
        f"{job} must pass install-binstall: 'false', or setup-rust installs a "
        "second cargo-binstall"
    )
    # sccache cannot cache incremental compilation, and Cargo enables it by
    # default for dev profiles.
    env = job.body.get("env")
    assert isinstance(env, dict), f"{job} must declare a job-level env block"
    assert env.get("CARGO_INCREMENTAL") == "0", (
        f"{job} must disable incremental compilation for sccache"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_each_job_demands_the_backend_its_placement_allows(job: Job) -> None:
    """`ubicloud` fails a proxy-less job loudly; `any` spares a hosted arm."""
    expected = expected_expect_cache(job.runner_labels)
    inputs = _setup(job).get("with") or {}
    assert isinstance(inputs, dict)
    assert inputs.get("expect-cache") == expected, (
        f"{job} runs on {job.runner_summary}, so it must pass "
        f"expect-cache: {expected}, not {inputs.get('expect-cache')!r}"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_no_retired_piece_survives(job: Job) -> None:
    """A caller's wrapper or switch overrides `setup-rust` without a word."""
    findings = retired_findings(job.body.get("env"), job.steps)
    assert not findings, f"{job} still hand-rolls sccache: {findings}"


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_setup_rust_precedes_every_build(job: Job) -> None:
    """A build before `setup-rust` starts the server bypasses the cache."""
    names = [str(step.get("name", step.get("uses", ""))) for step in job.steps]
    setup_at = names.index(SETUP_STEP)
    first_build = next(
        (
            index
            for index, step in enumerate(job.steps)
            if any(p.search(str(step.get("run", ""))) for p in COMPILING_COMMANDS)
        ),
        None,
    )
    assert first_build is not None, f"{job} compiles nothing"
    assert setup_at < first_build, (
        f"{job} runs a build before setup-rust starts sccache, so that build "
        "bypasses the compiler cache"
    )


@pytest.mark.parametrize("job", WRAPPED, ids=_ids(WRAPPED))
def test_statistics_are_reported_even_when_the_build_fails(job: Job) -> None:
    """Report the hit rate unconditionally, or a broken cache stays invisible."""
    report = next(step for step in job.steps if step.get("name") == REPORT_STEP)
    assert str(report.get("if", "")).strip() == "always()", (
        f"{job} must report sccache statistics with `if: always()`; a wrapper "
        "doing nothing is most visible on the run that fails"
    )
    body = str(report.get("run", ""))
    assert "--show-stats" in body, f"{job} must run sccache --show-stats"
    assert "GITHUB_STEP_SUMMARY" in body, (
        f"{job} must write the statistics to the job summary"
    )
    # The job summary is not readable through the REST API, so the statistics
    # must also reach the log, where anyone can confirm the hit rate or a read
    # or write error after the run.
    assert "printf '%s\\n' \"$stats\"" in body, (
        f"{job} must print the statistics to the log as well as the summary"
    )
    # `Cache location` reads `ghac` for the proxy and GitHub's own service
    # alike, so the report must name the backend setup-rust chose.
    backend = f"steps.{SETUP_ID}.outputs.cache-backend"
    assert backend in str(report.get("env", {})), f"{job} must report {backend}"
    assert "printf -- '- backend: %s\\n\\n' \"${SCCACHE_BACKEND:-none}\"" in body, (
        f"{job} must print the selected backend to the job summary"
    )


def _demands_the_proxy(step: dict[str, object]) -> bool:
    """Report whether a step is a `Setup Rust` call demanding Ubicloud's proxy."""
    inputs = step.get("with")
    return (
        SETUP_RUST.match(str(step.get("uses", ""))) is not None
        and isinstance(inputs, dict)
        and inputs.get("expect-cache") == "ubicloud"
    )


def test_github_hosted_jobs_demand_no_proxy() -> None:
    """A job that never reaches Ubicloud must not demand its cache proxy."""
    offenders = [
        str(job)
        for job in ALL_JOBS
        if not job.uses_ubicloud and any(_demands_the_proxy(s) for s in job.steps)
    ]
    assert not offenders, f"{offenders} are not on Ubicloud but demand its cache proxy"


@pytest.mark.parametrize(
    ("env", "steps", "expected"),
    [
        pytest.param({"SCCACHE_GHA_ENABLED": "true"}, [], 1, id="backend-switch"),
        pytest.param(
            {},
            [{"name": "Export the Actions cache endpoint for sccache"}],
            1,
            id="export",
        ),
        pytest.param({}, [{"env": {"RUSTC_WRAPPER": "sccache"}}], 1, id="step-wrapper"),
        pytest.param(
            {}, [{"env": {"CARGO_INCREMENTAL": "0"}}], 0, id="step-incremental"
        ),
        pytest.param({}, [{"name": "Install sccache"}], 1, id="install"),
        pytest.param(
            {},
            [
                {
                    "name": "Tools",
                    "uses": "taiki-e/install-action@x",
                    "with": {"tool": "sccache@0.16.0"},
                }
            ],
            1,
            id="renamed-installer",
        ),
        pytest.param(
            {},
            [{"name": "Tools", "uses": "mozilla-actions/sccache-action@x"}],
            1,
            id="renamed-mozilla-installer",
        ),
        pytest.param(
            {},
            [
                {
                    "name": "Tools",
                    "uses": "taiki-e/install-action@x",
                    "with": {"tool": "cargo-binstall"},
                }
            ],
            0,
            id="other-tool-is-not-sccache",
        ),
        pytest.param(
            {},
            [
                {
                    "name": "Setup",
                    "run": 'echo "ACTIONS_CACHE_URL=$ACTIONS_CACHE_URL" >> "$GITHUB_ENV"',
                }
            ],
            1,
            id="renamed-endpoint-export",
        ),
        pytest.param(
            {},
            [
                {
                    "name": "Setup",
                    "uses": "actions/github-script@x",
                    "with": {
                        "script": "core.exportVariable('ACTIONS_RUNTIME_TOKEN', t)"
                    },
                }
            ],
            1,
            id="renamed-script-export",
        ),
        pytest.param({}, [{"run": "sccache --start-server"}], 1, id="server-start"),
        pytest.param(
            {},
            [
                {
                    "uses": "actions/github-script@x",
                    "with": {"script": "ACTIONS_CACHE_SERVICE_V2"},
                }
            ],
            1,
            id="flag-rewrite",
        ),
        pytest.param(
            {}, [{"name": REPORT_STEP, "run": "sccache --show-stats"}], 0, id="report"
        ),
        pytest.param({"CARGO_INCREMENTAL": "0"}, [], 0, id="incremental-off"),
    ],
)
def test_the_retired_piece_reader_is_narrow_as_well_as_sufficient(
    env: dict[str, str], steps: list[dict[str, object]], expected: int
) -> None:
    """Each retired form is caught, and the report and other settings are not.

    The checked-in jobs can only show that the rule passes on them. These
    fixtures show that it would catch each retired form, and that it leaves
    the statistics report and `CARGO_INCREMENTAL` alone.
    """
    findings = retired_findings(env, steps)
    assert len(findings) == expected, (
        f"expected {expected} finding(s) for {env!r} and {steps!r}, got {findings!r}"
    )


@pytest.mark.parametrize(
    ("step", "expected"),
    [
        pytest.param(
            {
                "name": "Renamed",
                "uses": f"leynos/shared-actions/.github/actions/setup-rust@{'a' * 40}",
                "with": {"expect-cache": "ubicloud"},
            },
            True,
            id="renamed-call-still-demands-the-proxy",
        ),
        pytest.param(
            {
                "name": SETUP_STEP,
                "uses": "actions/checkout@v4",
                "with": {"expect-cache": "ubicloud"},
            },
            False,
            id="other-action-is-not-setup-rust",
        ),
        pytest.param(
            {
                "name": SETUP_STEP,
                "uses": f"leynos/shared-actions/.github/actions/setup-rust@{'a' * 40}",
                "with": {"expect-cache": "any"},
            },
            False,
            id="any-demands-nothing",
        ),
    ],
)
def test_the_proxy_demand_is_read_from_the_action_not_the_step_name(
    step: dict[str, object], expected: bool
) -> None:
    """Identify a `setup-rust` call by `uses`, so renaming its step hides nothing."""
    assert _demands_the_proxy(step) is expected
