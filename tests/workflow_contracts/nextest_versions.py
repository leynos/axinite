"""Which cargo-nextest release each workflow installs.

Separated from ``nextest_boundary_test`` so the question has two halves
that can be stated apart. `installed_versions_in` is pure: it takes each
workflow's text by file name and answers from that alone, so a test can
hand it a literal pin, a deferred reference, or a lane that defers to a
variable it never declares, without writing a file. `read_workflow_texts`
is the acquisition: it lists the workflow directory and reads each file
through `read_source`, so a file that cannot be read is reported as a
`SourceReadError` naming it rather than as an `OSError` from inside a
function whose name promises a mapping.

Every workflow that installs the runner is reported, not only those
declaring ``CARGO_NEXTEST_VERSION``. Four lanes install it through the
shared tool action and defer to that variable; ``mutation-testing.yml``
pins the version literally inside the ``setup-commands:`` it hands to the
reusable mutation workflow. A reading confined to the ``env`` block saw
four of the five, so the literal could drift to another release and the
agreement assertion would still pass over the four that agreed.
"""

import re
import typing as typ

from _workflow_policy import WORKFLOW_DIR, parse_workflow, workflow_paths
from contract_sources import read_source

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path

#: Stands for a lane that defers to ``CARGO_NEXTEST_VERSION`` and
#: declares it at neither scope. Reported rather than skipped: the
#: expression resolves to the empty string and the lane installs
#: whatever the tool action then picks.
UNDECLARED: typ.Final[str] = "<undeclared>"

#: A ``cargo-nextest@<version>`` reference that pins a literal version.
#: Matched against the workflow's raw text rather than its parsed
#: document, because one lane pins the version inside a
#: ``setup-commands:`` block handed to a reusable workflow, where no
#: key names it and no ``env`` declares it. The character class stops
#: before ``$`` so an expression is not mistaken for a version, and the
#: lookahead refuses a literal prefix that an expression completes:
#: ``cargo-nextest@0.9.${{ matrix.patch }}`` would otherwise read as the
#: version ``0.9.`` and count as resolved.
_NEXTEST_LITERAL: typ.Final[re.Pattern[str]] = re.compile(
    r"cargo-nextest@(?![^\s$'\"]*\$)([^\s$'\"]+)"
)

#: A ``cargo-nextest@`` reference that defers to ``CARGO_NEXTEST_VERSION``.
#: The variable may be declared at workflow or job scope, so the
#: reference is recognized here and resolved against both below.
_NEXTEST_DEFERRED: typ.Final[re.Pattern[str]] = re.compile(
    r"cargo-nextest@\$\{\{\s*env\.CARGO_NEXTEST_VERSION\s*\}\}"
)

#: Any ``cargo-nextest@`` reference at all, literal or deferred. What
#: makes a workflow one that installs the runner.
_NEXTEST_INSTALL: typ.Final[str] = "cargo-nextest@"


def declared_env_versions(document: object) -> set[str]:
    """Return the ``CARGO_NEXTEST_VERSION`` values a workflow declares.

    Both scopes are read. Four lanes here declare it at workflow level
    and one inside its job, and a reading confined to the workflow block
    reported that job's lane as deferring to a variable nobody set while
    it had in fact installed the runner.

    Parameters
    ----------
    document
        A parsed workflow document.

    Returns
    -------
    set of str
        Every value declared, at either scope. Empty when none is.
    """
    if not isinstance(document, dict):
        return set()
    blocks = [document.get("env")]
    jobs = document.get("jobs")
    if isinstance(jobs, dict):
        blocks.extend(
            job.get("env") for job in jobs.values() if isinstance(job, dict)
        )
    return {
        str(block["CARGO_NEXTEST_VERSION"])
        for block in blocks
        if isinstance(block, dict) and "CARGO_NEXTEST_VERSION" in block
    }


def _versions_in(name: str, text: str) -> set[str]:
    """Return the versions one installing workflow's text pins or declares.

    A reference to the workflow's own ``env`` resolves to the declared
    values, so a lane deferring to a variable it never sets reads as
    installing `UNDECLARED` rather than as installing nothing. So does a
    reference neither pattern accounts for, such as
    ``cargo-nextest@${{ inputs.nextest }}``: every install reference is
    counted, and one left over is unresolved rather than silently empty.

    Parameters
    ----------
    name
        The workflow's file name, quoted if its text does not parse.
    text
        The workflow's source.

    Returns
    -------
    set of str
        The literal pins, plus the declared values if it defers.
    """
    literals = _NEXTEST_LITERAL.findall(text)
    deferred = len(_NEXTEST_DEFERRED.findall(text))
    versions = set(literals)
    if deferred:
        declared = declared_env_versions(parse_workflow(text, name))
        versions |= declared or {UNDECLARED}
    if text.count(_NEXTEST_INSTALL) > len(literals) + deferred:
        versions.add(UNDECLARED)
    return versions


def installed_versions_in(sources: "Mapping[str, str]") -> dict[str, set[str]]:
    """Return the cargo-nextest versions each workflow installs.

    Pure: the answer comes from the text it is handed, and nothing is
    read here.

    Parameters
    ----------
    sources
        Each workflow's text, keyed by file name.

    Returns
    -------
    dict of str to set of str
        Workflow file name to the versions it installs, for those that
        install the runner at all.

    Raises
    ------
    AssertionError
        If a workflow that defers to the variable does not parse as a
        mapping, so its ``env`` blocks cannot be read.

    Examples
    --------
    >>> installed_versions_in({"a.yml": "run: cargo install cargo-nextest@0.9.1"})
    {'a.yml': {'0.9.1'}}
    """
    return {
        name: _versions_in(name, text)
        for name, text in sources.items()
        if _NEXTEST_INSTALL in text
    }


def read_workflow_texts(directory: "Path" = WORKFLOW_DIR) -> dict[str, str]:
    """Read every workflow in a directory, keyed by file name.

    Acquisition, not a query: this lists a directory and reads each file.

    Parameters
    ----------
    directory
        Directory to scan. It defaults to the repository's workflow
        directory; the parameter exists so a test can point the same read
        at a temporary tree.

    Returns
    -------
    dict of str to str
        Each workflow's text, keyed by file name, in name order.

    Raises
    ------
    SourceReadError
        If the directory cannot be listed, an entry in it cannot be
        classified, or a workflow cannot be read or is not UTF-8.
    """
    return {path.name: read_source(path) for path in workflow_paths(directory)}
