"""Readers for the rule that only `main` publishes coverage to CodeScene.

The rule has two halves. Nothing a pull request can start may reach
CodeScene: no token, no uploader, no `codescene.io`, whether written in the
workflow the pull request starts or in any workflow it calls. And exactly one
push-to-`main` publisher uploads, guarded so a dispatch from another branch
cannot upload and a run is never cancelled half way.

Each reader here answers one question about parsed workflows and reads no
file; `_strict_workflows.py` does the reading. So
`coverage_publication_reader_test.py` can drive every clause with a
constructed estate. That matters because the
repository's own workflows are correct: a reader parametrized only over them
would pass with its protection deleted.
"""

from __future__ import annotations

import fnmatch
import typing as typ

from _strict_workflows import WorkflowReadError, Workflows, jobs, triggers

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Callable, Iterator, Mapping

    #: One step of a closure: the workflows a set reaches directly.
    Widening = Callable[[Workflows, set[str]], set[str]]

#: This repository, as a `uses:` reference would spell it.
REPOSITORY = "leynos/axinite"

#: Where a local reusable workflow lives, relative to the repository root.
WORKFLOW_DIRECTORY = ".github/workflows/"

#: The events through which a pull request starts a workflow. Review events
#: and the merge queue run on pull-request content just as `pull_request`
#: does, so they seed the pull-request surface too.
PULL_REQUEST_EVENTS: frozenset[str] = frozenset(
    {
        "pull_request",
        "pull_request_target",
        "pull_request_review",
        "pull_request_review_comment",
        "merge_group",
    }
)

#: Text whose presence anywhere in a pull-request-reachable workflow means it
#: can reach CodeScene or its credential. Matched case-folded against every
#: key and scalar, so a `defaults.run.shell` wrapper or a `workflow_call`
#: secret declaration is read like any step.
FORBIDDEN_ON_PULL_REQUESTS: tuple[str, ...] = (
    "codescene.io",
    "cs_access_token",
    "upload-codescene-coverage",
    "cs-coverage",
)


def local_callee(uses: str) -> str | None:
    """Return the workflow file a job-level `uses:` calls in this repository.

    Matched by shape rather than by a list of prefixes: strip a leading `./`
    or `$/`, and a call is local when what remains is a path under the
    workflow directory. `$/` is GitHub's documented same-repository form and
    carries no ref.

    Parameters
    ----------
    uses
        A job's `uses:` value.

    Returns
    -------
    str or None
        The called file's name, or `None` for a call to another repository.

    Raises
    ------
    WorkflowReadError
        For a `$/` call with a ref, which GitHub rejects, and for a call to
        this repository qualified with a ref, which runs the file as it stood
        at that ref rather than as it stands here, so the closure cannot read
        what it runs.
    """
    path = _repository_path(uses.strip())
    if path is None or not path.startswith(WORKFLOW_DIRECTORY):
        return None
    return path.removeprefix(WORKFLOW_DIRECTORY).split("@", 1)[0]


def _repository_path(reference: str) -> str | None:
    """Return a same-repository reference's path, or `None` for another repo."""
    # GitHub reads the owner and repository names case-insensitively, so
    # `Leynos/Axinite/...` is this repository too.
    if reference.casefold().startswith(f"{REPOSITORY}/".casefold()):
        raise WorkflowReadError(f"qualified self-call {reference!r}; use `./` or `$/`")
    if reference.startswith("$/"):
        if "@" in reference:
            raise WorkflowReadError(f"`$/` call with a ref: {reference!r}")
        return reference.removeprefix("$/")
    return reference.removeprefix("./") if reference.startswith("./") else None


def callees(document: Mapping[object, object]) -> set[str]:
    """Return the local workflows a workflow's jobs call."""
    called = (job.get("uses") for job in jobs(document).values())
    found = {local_callee(uses) for uses in called if isinstance(uses, str)}
    return {callee for callee in found if callee is not None}


def _chained_names(configuration: object) -> set[str]:
    """Return the workflow names a `workflow_run` trigger chains onto."""
    if not isinstance(configuration, dict):
        return set()
    match configuration.get("workflows"):
        case str() as name:
            return {name}
        case list() as names:
            return {name for name in names if isinstance(name, str)}
        case _:
            return set()


def _closed(workflows: Workflows, seed: set[str], widen: Widening) -> set[str]:
    """Widen a set of workflows by one step at a time until it stops growing."""
    surface = seed
    while (
        widened := surface | (widen(workflows, surface) & set(workflows))
    ) != surface:
        surface = widened
    return surface


def _called_by(workflows: Workflows, surface: set[str]) -> set[str]:
    """Return the local workflows any workflow in a set calls."""
    return {callee for name in surface for callee in callees(workflows[name])}


def _chained_onto(workflows: Workflows, surface: set[str]) -> set[str]:
    """Return the workflows a `workflow_run` trigger chains onto the set."""
    names = {workflows[name].get("name") for name in surface}
    return {
        name
        for name, document in workflows.items()
        if _chained_names(triggers(document).get("workflow_run")) & names
    }


def _pull_request_step(workflows: Workflows, surface: set[str]) -> set[str]:
    """Widen the pull-request surface by chains and calls, one step."""
    return _chained_onto(workflows, surface) | _called_by(workflows, surface)


def pull_request_surface(workflows: Workflows) -> set[str]:
    """Return every workflow a pull request can cause to run.

    Seeded by the pull-request events, widened by `workflow_run` chains onto
    a seeded workflow's `name:`, and closed over local calls, repeated until
    nothing new is added.

    Parameters
    ----------
    workflows
        The parsed estate.

    Returns
    -------
    set of str
        File names of every pull-request-reachable workflow.
    """
    seed = {
        name
        for name, document in workflows.items()
        if PULL_REQUEST_EVENTS & set(triggers(document))
    }
    return _closed(workflows, seed, _pull_request_step)


def scalars(node: object) -> Iterator[str]:
    """Yield every key and scalar in a parsed document, as text."""
    match node:
        case dict():
            yield from _mapping_scalars(node)
        case list():
            yield from (text for item in node for text in scalars(item))
        case None:
            return
        case _:
            yield str(node)


def _mapping_scalars(node: dict[object, object]) -> Iterator[str]:
    """Yield a mapping's keys and every scalar beneath its values."""
    for key, value in node.items():
        yield str(key)
        yield from scalars(value)


def pull_request_faults(workflows: Workflows) -> list[str]:
    """Return every way the pull-request surface can reach CodeScene.

    Parameters
    ----------
    workflows
        The parsed estate.

    Returns
    -------
    list of str
        One sentence per offending workflow and text, and one per job that
        forwards every secret with `secrets: inherit`.
    """
    return [
        fault
        for name in sorted(pull_request_surface(workflows))
        for fault in _workflow_faults(name, workflows[name])
    ]


def _workflow_faults(name: str, document: Mapping[object, object]) -> list[str]:
    """Return one pull-request-reachable workflow's route to CodeScene."""
    folded = " ".join(scalars(document)).casefold()
    mentions = [
        f"{name} is reachable from a pull request and mentions {needle!r}"
        for needle in FORBIDDEN_ON_PULL_REQUESTS
        if needle in folded
    ]
    inherited = [
        f"{name}:{job_id} forwards every secret with `secrets: inherit`"
        for job_id, job in jobs(document).items()
        if job.get("secrets") == "inherit"
    ]
    return mentions + inherited


def pushes_to_main(document: Mapping[object, object]) -> bool:
    """Report whether a workflow runs on a push to `main`.

    Branch filters are globs, with `!` negation and `branches-ignore`
    honoured; a push filtered to tags alone reaches no branch.
    """
    events = triggers(document)
    if "push" not in events:
        return False
    configuration = events["push"]
    if not isinstance(configuration, dict):
        return True
    ignored = configuration.get("branches-ignore")
    if isinstance(ignored, list):
        return not any(fnmatch.fnmatchcase("main", str(glob)) for glob in ignored)
    included = configuration.get("branches")
    if isinstance(included, list):
        return _globs_select_main([str(glob) for glob in included])
    return "tags" not in configuration and "tags-ignore" not in configuration


def _globs_select_main(globs: list[str]) -> bool:
    """Apply a `branches` list in order, later `!` entries overriding earlier."""
    selected = False
    for glob in globs:
        negated = glob.startswith("!")
        if fnmatch.fnmatchcase("main", glob.removeprefix("!")):
            selected = not negated
    return selected


def push_surface(workflows: Workflows) -> set[str]:
    """Return every workflow a push to `main` can cause to run."""
    seed = {name for name, document in workflows.items() if pushes_to_main(document)}
    return _closed(workflows, seed, _called_by)


def upload_guard_faults(condition: str, required: tuple[str, ...]) -> list[str]:
    """Return what is wrong with an upload step's `if:` guard.

    Split on `&&` and compared conjunct by conjunct; each required term must
    appear whole. Extra conjuncts narrow the guard and are allowed, which is
    why a `||` anywhere is refused outright: hidden in an extra conjunct it
    would widen the guard while every required term still read as present.

    Parameters
    ----------
    condition
        The step's `if:` expression.
    required
        The conjuncts the guard must contain.

    Returns
    -------
    list of str
        One sentence per fault, empty when the guard is acceptable.
    """
    collapsed = " ".join(condition.split())
    if "||" in collapsed:
        return [f"the guard {collapsed!r} contains `||`, which can widen it"]
    terms = {term.strip() for term in collapsed.split("&&")}
    return [
        f"the guard does not require {term!r}" for term in required if term not in terms
    ]
