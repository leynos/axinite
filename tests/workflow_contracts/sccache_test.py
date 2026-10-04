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
import types
import typing as typ
from collections import abc

import pytest
from _workflow_files import jobs
from _workflow_policy import Job

ALL_JOBS: tuple[Job, ...] = tuple(jobs())

SETUP_STEP = "Setup Rust"
SETUP_ID = "setup-rust"
REPORT_STEP = "Report sccache statistics"

#: The shared action that owns sccache, pinned to a full commit.
#: The `setup-rust` commit every call must pin. `6cec89ba` is the first to give
#: the sccache server a 60 s start-up timeout and let it fail open
#: (shared-actions #546); an older pin hard-fails the job at `Setup Rust` when
#: the cache probe is slow. Raise it deliberately, never by a stray repin.
SETUP_RUST_PIN = "6cec89bac47a21cf756d68d638a9a510998e57f8"
SETUP_RUST_ACTION = re.compile(r"^leynos/shared-actions/\.github/actions/setup-rust@")
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


def _step_env_without_clearing(step: dict[str, object]) -> object:
    """Return a step's `env`, minus a `RUSTC_WRAPPER` set to the empty string.

    An empty `RUSTC_WRAPPER` counts as unset to Cargo, so a step that sets it
    is opting that step out of the wrapper, not installing a wrapper of its own.
    """
    env = step.get("env")
    if not isinstance(env, dict) or env.get("RUSTC_WRAPPER") != "":
        return env
    return {name: value for name, value in env.items() if name != "RUSTC_WRAPPER"}


def _retired_in_step(index: int, step: dict[str, object]) -> list[str]:
    """Return every retired piece of sccache wiring in one step."""
    findings = _retired_variables(f"step {index}", _step_env_without_clearing(step))
    if step.get("name") in RETIRED_STEPS:
        findings.append(f"step {index} is the retired {step['name']!r}")
    if _installs_sccache(step):
        findings.append(f"step {index} installs sccache itself")
    if _exports_cache_endpoint(step):
        findings.append(f"step {index} republishes the cache endpoint itself")
    if SERVER_COMMAND.search(str(step.get("run", ""))):
        findings.append(f"step {index} starts or zeroes the sccache server")
    if "ACTIONS_CACHE_SERVICE_V2" in f"{step.get('with', '')} {step.get('run', '')}":
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


def _pins_setup_rust(uses: str) -> bool:
    """Report whether a `uses` value pins `setup-rust` to `SETUP_RUST_PIN`."""
    return uses.endswith(f"/setup-rust@{SETUP_RUST_PIN}") and bool(
        SETUP_RUST_ACTION.match(uses)
    )


def _setup(job: Job) -> dict[str, object]:
    """Return a job's one `setup-rust` call, named `Setup Rust`.

    Calls are counted by action reference, so a second call under another
    name is still a second call; the display name is checked separately.
    """
    calls = [
        step for step in job.steps if SETUP_RUST_ACTION.match(str(step.get("uses", "")))
    ]
    assert len(calls) == 1, f"{job} must call setup-rust exactly once"
    assert calls[0].get("name") == SETUP_STEP, (
        f"{job} must name its setup-rust call {SETUP_STEP!r}"
    )
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
    assert _pins_setup_rust(str(step.get("uses", ""))), (
        f"{job} must pin setup-rust to {SETUP_RUST_PIN}, the first commit whose "
        "sccache start-up fails open"
    )
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
    report_env = report.get("env")
    assert isinstance(report_env, dict), f"{job} must give its report step an env"
    assert report_env.get("SCCACHE_BACKEND") == f"${{{{ {backend} }}}}", (
        f"{job} must bind SCCACHE_BACKEND to {backend}"
    )
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
        pytest.param({}, [{"env": {"RUSTC_WRAPPER": ""}}], 0, id="step-clears-wrapper"),
        pytest.param(
            {},
            [{"env": {"RUSTC_WRAPPER": "", "SCCACHE_DIR": "/x"}}],
            1,
            id="clear-hides-nothing-else",
        ),
        pytest.param({}, [{"name": "Install sccache"}], 1, id="install"),
        pytest.param(
            {},
            [
                {
                    "name": "Tweak",
                    "run": 'echo ACTIONS_CACHE_SERVICE_V2=false >> "$GITHUB_ENV"',
                }
            ],
            1,
            id="renamed-shell-flag-rewrite",
        ),
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


NESTED_BUILD_JOBS = tuple(
    job
    for job in WRAPPED
    if any("make test-workspace" in str(step.get("run", "")) for step in job.steps)
)


def test_the_nested_build_jobs_are_not_empty() -> None:
    """Guard against a selector that quietly matches nothing."""
    assert NESTED_BUILD_JOBS, "no job runs the trybuild-bearing workspace suite"


@pytest.mark.parametrize("job", NESTED_BUILD_JOBS, ids=_ids(NESTED_BUILD_JOBS))
def test_the_test_run_clears_the_wrapper_after_a_wrapped_build(job: Job) -> None:
    """trybuild's nested builds must not route through sccache.

    Each fixture build asks the sccache server for a compile it will not reuse,
    and routed that way the compile-contract sessions timed out. The workspace
    is built first, through the wrapper, so the cache still serves it.
    """
    run_at = next(
        i
        for i, step in enumerate(job.steps)
        if "make test-workspace" in str(step.get("run", ""))
    )
    env = job.steps[run_at].get("env")
    assert isinstance(env, dict) and env.get("RUSTC_WRAPPER") == "", (
        f"{job} must set RUSTC_WRAPPER to the empty string on the suite step"
    )
    build_at = next(
        (
            i
            for i, step in enumerate(job.steps)
            if re.search(r"\bcargo\s+build\b.*--tests\b", str(step.get("run", "")))
        ),
        None,
    )
    assert build_at is not None and build_at < run_at, (
        f"{job} must build the tests through the wrapper before the unwrapped run"
    )
    assert "RUSTC_WRAPPER" not in (job.steps[build_at].get("env") or {}), (
        f"{job} must leave the wrapper on for the build step"
    )


def _job_with_calls(*names: str) -> Job:
    """Return a stub job whose steps are `setup-rust` calls under `names`."""
    uses = f"leynos/shared-actions/.github/actions/setup-rust@{'a' * 40}"
    steps = [{"name": name, "uses": uses} for name in names]
    return typ.cast("Job", types.SimpleNamespace(steps=steps))


@pytest.mark.parametrize(
    ("names", "fragment"),
    [
        pytest.param((SETUP_STEP, "Cache"), "exactly once", id="second-call-renamed"),
        pytest.param(("Cache",), "must name", id="only-call-renamed"),
        pytest.param((), "exactly once", id="no-call"),
    ],
)
def test_the_setup_reader_counts_calls_by_action_not_by_name(
    names: tuple[str, ...], fragment: str
) -> None:
    """A renamed or extra `setup-rust` call is a finding, not an exemption."""
    with pytest.raises(AssertionError, match=fragment):
        _setup(_job_with_calls(*names))


@pytest.mark.parametrize(
    ("uses", "expected"),
    [
        pytest.param(
            f"leynos/shared-actions/.github/actions/setup-rust@{SETUP_RUST_PIN}",
            True,
            id="the-pinned-commit",
        ),
        pytest.param(
            f"leynos/shared-actions/.github/actions/setup-rust@{'4fb8eb7a' + 'a' * 32}",
            False,
            id="an-older-commit",
        ),
        pytest.param(
            f"leynos/shared-actions/.github/actions/setup-rust@{SETUP_RUST_PIN[:8]}",
            False,
            id="an-abbreviated-pin",
        ),
        pytest.param(
            f"other/shared-actions/.github/actions/setup-rust@{SETUP_RUST_PIN}",
            False,
            id="another-owner",
        ),
    ],
)
def test_only_the_pinned_setup_rust_commit_is_accepted(
    uses: str, expected: bool
) -> None:
    """An older, abbreviated or foreign pin must not satisfy the pin contract."""
    assert _pins_setup_rust(uses) is expected
