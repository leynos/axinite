"""What each workflow lane actually runs, resolved leg by leg.

This reads a workflow the way GitHub dispatches it: a job's guard against the
event, its matrix legs, each step's script with the leg substituted in, and
then every Cargo or Make command in that script. The result is a `SuiteRun`
per leg per command, keyed by the work it does rather than the job it sits in,
which is what lets `suite_duplication_test.py` ask whether two lanes run the
same suite.

See `_suite_keys.py` for what a single command selects, and
`_suite_targets.py` for what a Make target and the manifest contribute.
"""

from __future__ import annotations

import re
import typing as typ
from dataclasses import dataclass
from types import MappingProxyType

import yaml

from _sources import SourceError, read_text
from _suite_keys import feature_key, profile_of
from _suite_targets import DEFAULT_PROFILE, MAKE_COMMAND, MAKE_TARGETS, WORKSPACE
from _workflow_policy import (
    DIST_GENERATED,
    WORKFLOW_DIR,
    Job,
    jobs_of,
    matrix_legs,
    parse_workflow,
    runs_on_event,
    step_text,
    triggers,
    workflow_paths,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator, Mapping
    from pathlib import Path

#: Every workflow this contract judges, parsed and keyed by name. The reading
#: is a mapping the caller cannot alter: one contract mutating the estate
#: would change what a later one judges, and the failure would name the later
#: contract.
Estate: typ.TypeAlias = "Mapping[str, Mapping[str, object]]"

#: The triggers a developer waits on, and the only ones that reach a paid
#: runner. A scheduled duplicate is free and blocks nobody.
PAID_EVENTS: tuple[str, ...] = ("pull_request", "push")

#: Cargo commands that run a suite. The instrumented form counts:
#: `cargo llvm-cov nextest` compiles and runs the same tests as
#: `cargo nextest run`, which is the whole reason a coverage lane can stand in
#: for a test lane. `cargo llvm-cov clean` and `cargo llvm-cov report` are
#: excluded by requiring `nextest` in the same command.
CARGO_COMMANDS: tuple[re.Pattern[str], ...] = (
    re.compile(r"\bcargo\s+(?:\+\S+\s+)?llvm-cov\s+nextest\b(?P<args>[^\n]*)"),
    re.compile(r"\bcargo\s+(?:\+\S+\s+)?nextest\s+run\b(?P<args>[^\n]*)"),
    re.compile(r"\bcargo\s+(?:\+\S+\s+)?test\b(?P<args>[^\n]*)"),
)

#: Where a Cargo command names the crate it runs against.
MANIFEST_RE: re.Pattern[str] = re.compile(r"--manifest-path[= ](?P<path>\S+)")

#: `${{ matrix.<key> }}`, which is how a leg's flags reach the command.
MATRIX_REFERENCE_RE: re.Pattern[str] = re.compile(
    r"\$\{\{\s*matrix\.(?P<key>[A-Za-z0-9_-]+)\s*\}\}"
)


@dataclass(frozen=True)
class SuiteRun:
    """One execution of the workspace suite, with the features it selects.

    Attributes
    ----------
    job
        The job that runs it.
    leg
        The matrix leg's name, or an empty string when the job has no matrix.
    scope
        What the run covers: the workspace, or one out-of-workspace crate.
    profile
        The nextest profile, which decides which of the suite's tests run.
    features
        The feature selection, as a set, so that two spellings of one
        selection compare equal.
    """

    job: Job
    leg: str
    scope: str
    profile: str
    features: frozenset[str]

    @property
    def key(self) -> tuple[str, str, frozenset[str]]:
        """Return what makes two runs the same work.

        Returns
        -------
        tuple of (str, str, frozenset of str)
            The run's scope, its nextest profile, and the features it
            resolves to. The profile is part of the identity because it
            selects which tests run at all.
        """
        return self.scope, self.profile, self.features

    def __str__(self) -> str:
        """Identify the run in assertion output."""
        return f"{self.job} ({self.leg})" if self.leg else str(self.job)


def substitute(text: str, leg: dict[str, str]) -> str:
    """Return a step's script with this leg's matrix values in place.

    An unresolved reference is left as it stands rather than dropped, so a
    typo in a matrix key shows up in the key instead of quietly widening two
    different runs into one.

    Parameters
    ----------
    text
        The step's script, with `${{ matrix.<key> }}` references unresolved.
    leg
        One matrix leg's values, keyed by matrix key.

    Returns
    -------
    str
        The script with every reference this leg supplies substituted in.
    """
    return MATRIX_REFERENCE_RE.sub(lambda match: leg.get(match["key"], match[0]), text)


def cargo_runs(
    script: str, defaults: frozenset[str]
) -> Iterator[tuple[str, str, frozenset[str]]]:
    """Yield the scope and features of each Cargo suite command in a script.

    A command that names neither a manifest nor `--workspace` is skipped. It
    is a filtered or single-crate run, such as the WIT instantiation test, and
    counting it as the workspace suite would report a clash with a lane that
    runs thousands of tests it does not.

    Parameters
    ----------
    script
        A step's script, with every matrix reference already substituted.
    defaults
        The root manifest's `default` feature list, passed through to
        `feature_key` rather than read here.

    Yields
    ------
    tuple of str, str and frozenset of str
        The scope a command covers, the nextest profile it selects, and the
        features it enables, one tuple per suite command found.
    """
    for pattern in CARGO_COMMANDS:
        for match in pattern.finditer(script):
            args = match["args"]
            manifest = MANIFEST_RE.search(args)
            if manifest is not None:
                yield (
                    f"crate:{manifest['path']}",
                    profile_of(args),
                    feature_key(args, defaults),
                )
            elif "--workspace" in args:
                yield WORKSPACE, profile_of(args), feature_key(args, defaults)


def make_runs(
    script: str, defaults: frozenset[str]
) -> Iterator[tuple[str, str, frozenset[str]]]:
    """Yield the scope, profile and features of each Make target in a script.

    Only the workspace half reads the step's variables. `make test-github-tool`
    runs plain `cargo test` over one crate: it takes no features and no
    nextest profile, so giving it the step's would invent a difference between
    two identical runs.

    Parameters
    ----------
    script
        A step's script, with every matrix reference already substituted.
    defaults
        The root manifest's `default` feature list, passed through to
        `feature_key` rather than read here.

    Yields
    ------
    tuple of str, str and frozenset of str
        The scope, profile and features of each suite a Make target runs. A
        target that runs two suites yields two tuples.
    """
    for match in MAKE_COMMAND.finditer(script):
        for scope, takes_features in MAKE_TARGETS[match["target"]]:
            if takes_features:
                yield (
                    scope,
                    profile_of(match["args"]),
                    feature_key(match["args"], defaults),
                )
            else:
                yield scope, DEFAULT_PROFILE, frozenset()


def suite_runs_in(job: Job, event: str, defaults: frozenset[str]) -> Iterator[SuiteRun]:
    """Yield every suite run a job performs on an event.

    Parameters
    ----------
    job
        The job to read.
    event
        A `github.event_name` value, which selects the matrix legs.
    defaults
        The root manifest's `default` feature list.

    Yields
    ------
    SuiteRun
        One entry per leg per suite command, in workflow order.
    """
    for leg in matrix_legs(job, event):
        for step in job.steps:
            script = substitute(step_text(step), leg)
            for scope, profile, features in (
                *cargo_runs(script, defaults),
                *make_runs(script, defaults),
            ):
                yield SuiteRun(
                    job=job,
                    leg=leg.get("name", ""),
                    scope=scope,
                    profile=profile,
                    features=features,
                )


def dispatched_jobs(documents: Estate, event: str) -> Iterator[Job]:
    """Yield every job an event can dispatch.

    A workflow that does not declare the trigger contributes nothing, and
    neither does a job whose own guard excludes it.

    Parameters
    ----------
    documents
        Parsed workflows, keyed by file name.
    event
        The trigger to resolve against.

    Yields
    ------
    Job
        Each job the event reaches, in workflow order.
    """
    for name, document in documents.items():
        if event not in triggers(document):
            continue
        yield from (job for job in jobs_of(name, document) if runs_on_event(job, event))


def suite_runs_for(
    documents: Estate, event: str, defaults: frozenset[str]
) -> list[SuiteRun]:
    """Return every suite run an event dispatches.

    Parameters
    ----------
    documents
        Parsed workflows, keyed by file name.
    event
        The trigger to resolve against.
    defaults
        The root manifest's `default` feature list, from
        `_suite_targets.read_default_features`. It is an argument because a
        run's identity depends on it and because reading it is a filesystem
        access, which belongs at the test entry point and not in here.

    Returns
    -------
    list of SuiteRun
        One entry per leg per suite command, in workflow order.
    """
    return [
        run
        for job in dispatched_jobs(documents, event)
        for run in suite_runs_in(job, event, defaults)
    ]


def duplicates_in(
    runs: list[SuiteRun],
) -> dict[tuple[str, str, frozenset[str]], list[SuiteRun]]:
    """Return the work more than one run covers.

    Parameters
    ----------
    runs
        Every suite run one trigger dispatches.

    Returns
    -------
    dict
        Each key covered by more than one run, mapped to the runs covering
        it. A key covered once is absent, so an empty mapping is the passing
        case.
    """
    by_key: dict[tuple[str, str, frozenset[str]], list[SuiteRun]] = {}
    for run in runs:
        by_key.setdefault(run.key, []).append(run)
    return {key: found for key, found in by_key.items() if len(found) > 1}


def read_estate(directory: Path = WORKFLOW_DIR) -> Estate:
    """Read and parse every workflow this contract judges.

    The boundary for the workflow side, matching
    `_suite_targets.read_default_features` on the manifest side. Reading here
    rather than at import is what keeps a workflow this cannot parse from
    becoming a collection error: a contract directory that fails to collect
    reports no failures at all, which reads exactly like a clean run.

    Parameters
    ----------
    directory
        The directory to scan. It defaults to the estate's own; the parameter
        exists so the failure cases can be stated against a temporary tree.

    Returns
    -------
    Mapping
        Each workflow document, keyed by file name, in a mapping the caller
        cannot alter. The dist-generated release workflow is left out: its
        contents are regenerated wholesale and nothing here may judge them.

    Raises
    ------
    SourceError
        If the directory cannot be scanned, or a workflow in it cannot be
        read or parsed as YAML.
    """
    try:
        paths = workflow_paths(directory)
    except OSError as error:
        raise SourceError(
            directory, f"cannot be scanned ({error.strerror or error})"
        ) from error
    documents: dict[str, Mapping[str, object]] = {}
    for path in paths:
        if path.name == DIST_GENERATED:
            continue
        text = read_text(path)
        try:
            documents[path.name] = parse_workflow(text, path.name)
        except yaml.YAMLError as error:
            raise SourceError(path, f"is not valid YAML ({error})") from error
    return MappingProxyType(documents)
