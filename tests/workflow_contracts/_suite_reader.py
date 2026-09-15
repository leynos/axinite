"""What each workflow lane actually runs, resolved leg by leg.

This reads a workflow the way GitHub dispatches it: a job's guard against the
event, its matrix legs, each step's script with the leg substituted in, and
then every Cargo or Make command in that script. The result is a `SuiteRun`
per leg per command, keyed by the work it does rather than the job it sits in,
which is what lets `suite_duplication_test.py` ask whether two lanes run the
same suite.

Feature sets are compared as sets, so a leg rewritten from `--features a,b` to
`--features a --features b` is still the same run, and `--all-features` stays
distinct from a list that happens to name every feature today, because
tomorrow it will not. A command that does not pass `--no-default-features` is
keyed with the root manifest's `default` list folded in, because Cargo enables
those whether the command names them or not: without that, a leg naming three
members of `default` read as different work from the leg naming none, and
`test.yml` ran both.

See `_suite_targets.py` for what a Make target and the manifest contribute.
"""

from __future__ import annotations

import re
import shlex
import typing as typ
from dataclasses import dataclass

from _suite_targets import (
    DEFAULT_FEATURES,
    DEFAULT_PROFILE,
    FEATURE_VARIABLE,
    MAKE_COMMAND,
    MAKE_TARGETS,
    PROFILE_VARIABLE,
    WORKSPACE,
)
from _workflow_policy import (
    DIST_GENERATED,
    Job,
    jobs_of,
    load,
    matrix_legs,
    runs_on_event,
    step_text,
    triggers,
    workflow_paths,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator

#: Sentinels for the flags that select features without naming any. They are
#: part of the key so that `--all-features` and `--no-default-features
#: --features libsql` cannot collide with each other or with a feature list.
ALL_FEATURES = ":all-features"
NO_DEFAULT_FEATURES = ":no-default-features"

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
        """Return what makes two runs the same work."""
        return self.scope, self.profile, self.features

    def __str__(self) -> str:
        """Identify the run in assertion output."""
        return f"{self.job} ({self.leg})" if self.leg else str(self.job)


def features_named_by(token: str, following: tuple[str, ...]) -> tuple[str, ...]:
    """Return the features one argument selects.

    Parameters
    ----------
    token
        One shell word of the command.
    following
        The word after it, when there is one. `--features` takes its value
        separately; the other spellings carry it.

    Returns
    -------
    tuple of str
        The feature names, or a sentinel for the flags that select features
        without naming any. Empty for every other argument: a profile, an
        output path or a test filter changes how a run is reported, not which
        tests it compiles and executes.
    """
    if token == "--all-features":
        return (ALL_FEATURES,)
    if token == "--no-default-features":
        return (NO_DEFAULT_FEATURES,)
    if token == "--features" and following:
        return tuple(following[0].split(","))
    if token.startswith("--features="):
        return tuple(token.partition("=")[2].split(","))
    return ()


def unpack_feature_variable(tokens: list[str]) -> list[str]:
    """Return the tokens with `TEST_FEATURES="..."` expanded in place.

    A `make` step passes the selection as one assignment word. Unpacking it
    here, rather than at the call site, means the two command shapes are keyed
    the same way and a coverage lane can be compared with a test lane.
    """
    return [
        part
        for token in tokens
        for part in (
            shlex.split(token.partition("=")[2])
            if token.startswith(f"{FEATURE_VARIABLE}=")
            else [token]
        )
    ]


def feature_key(args: str) -> frozenset[str]:
    """Return the feature selection a command's arguments make.

    Parameters
    ----------
    args
        The command's arguments, with every matrix reference already
        substituted.

    Returns
    -------
    frozenset of str
        The features the command actually enables: each named feature, the
        root manifest's defaults unless the command turns them off, and a
        sentinel for `--all-features` and for `--no-default-features`.
    """
    tokens = unpack_feature_variable(shlex.split(args, comments=False, posix=True))
    selected = {
        name
        for index, token in enumerate(tokens)
        for name in features_named_by(token, tuple(tokens[index + 1 : index + 2]))
    }
    named = frozenset(name for name in selected if name)
    # `--no-default-features` says the defaults are off, so the explicit list
    # is the whole of the selection. Everything else gets them whether it
    # names them or not, which is the point: a leg naming three members of
    # `default` is the leg that names nothing. `--all-features` needs no
    # exception, because its sentinel already keeps it apart from every list.
    if NO_DEFAULT_FEATURES in named:
        return named
    return named | DEFAULT_FEATURES


def profile_of(args: str) -> str:
    """Return the nextest profile a command's arguments select.

    Parameters
    ----------
    args
        The command's arguments, with every matrix reference already
        substituted. Both spellings are read: `--profile ci` on a Cargo
        command, and `NEXTEST_PROFILE=ci` on a Make target.

    Returns
    -------
    str
        The profile name, or the nextest default when the command names none.
    """
    tokens = shlex.split(args, comments=False, posix=True)
    for index, token in enumerate(tokens):
        if token == "--profile" and index + 1 < len(tokens):
            return tokens[index + 1]
        if token.startswith("--profile="):
            return token.partition("=")[2]
        if token.startswith(f"{PROFILE_VARIABLE}="):
            return token.partition("=")[2]
    return DEFAULT_PROFILE


def substitute(text: str, leg: dict[str, str]) -> str:
    """Return a step's script with this leg's matrix values in place.

    An unresolved reference is left as it stands rather than dropped, so a
    typo in a matrix key shows up in the key instead of quietly widening two
    different runs into one.
    """
    return MATRIX_REFERENCE_RE.sub(lambda match: leg.get(match["key"], match[0]), text)


def cargo_runs(script: str) -> Iterator[tuple[str, str, frozenset[str]]]:
    """Yield the scope and features of each Cargo suite command in a script.

    A command that names neither a manifest nor `--workspace` is skipped. It
    is a filtered or single-crate run, such as the WIT instantiation test, and
    counting it as the workspace suite would report a clash with a lane that
    runs thousands of tests it does not.
    """
    for pattern in CARGO_COMMANDS:
        for match in pattern.finditer(script):
            args = match["args"]
            manifest = MANIFEST_RE.search(args)
            if manifest is not None:
                yield f"crate:{manifest['path']}", profile_of(args), feature_key(args)
            elif "--workspace" in args:
                yield WORKSPACE, profile_of(args), feature_key(args)


def make_runs(script: str) -> Iterator[tuple[str, str, frozenset[str]]]:
    """Yield the scope, profile and features of each Make target in a script.

    Only the workspace half reads the step's variables. `make test-github-tool`
    runs plain `cargo test` over one crate: it takes no features and no
    nextest profile, so giving it the step's would invent a difference between
    two identical runs.
    """
    for match in MAKE_COMMAND.finditer(script):
        for scope, takes_features in MAKE_TARGETS[match["target"]]:
            if takes_features:
                yield scope, profile_of(match["args"]), feature_key(match["args"])
            else:
                yield scope, DEFAULT_PROFILE, frozenset()


def suite_runs_in(job: Job, event: str) -> Iterator[SuiteRun]:
    """Yield every suite run a job performs on an event."""
    for leg in matrix_legs(job, event):
        for step in job.steps:
            script = substitute(step_text(step), leg)
            for scope, profile, features in (
                *cargo_runs(script),
                *make_runs(script),
            ):
                yield SuiteRun(
                    job=job,
                    leg=leg.get("name", ""),
                    scope=scope,
                    profile=profile,
                    features=features,
                )


def dispatched_jobs(
    documents: dict[str, dict[str, object]], event: str
) -> Iterator[Job]:
    """Yield every job an event can dispatch.

    A workflow that does not declare the trigger contributes nothing, and
    neither does a job whose own guard excludes it.
    """
    for name, document in documents.items():
        if event not in triggers(document):
            continue
        yield from (job for job in jobs_of(name, document) if runs_on_event(job, event))


def suite_runs_for(
    documents: dict[str, dict[str, object]], event: str
) -> list[SuiteRun]:
    """Return every suite run an event dispatches.

    Parameters
    ----------
    documents
        Parsed workflows, keyed by file name.
    event
        The trigger to resolve against.

    Returns
    -------
    list of SuiteRun
        One entry per leg per suite command, in workflow order.
    """
    return [
        run
        for job in dispatched_jobs(documents, event)
        for run in suite_runs_in(job, event)
    ]


def duplicates_in(
    runs: list[SuiteRun],
) -> dict[tuple[str, str, frozenset[str]], list[SuiteRun]]:
    """Return the work more than one run covers."""
    by_key: dict[tuple[str, str, frozenset[str]], list[SuiteRun]] = {}
    for run in runs:
        by_key.setdefault(run.key, []).append(run)
    return {key: found for key, found in by_key.items() if len(found) > 1}


def load_estate() -> dict[str, dict[str, object]]:
    """Return every workflow this contract judges, parsed and keyed by name."""
    return {
        path.name: load(path)
        for path in workflow_paths()
        if path.name != DIST_GENERATED
    }


ESTATE = load_estate()
