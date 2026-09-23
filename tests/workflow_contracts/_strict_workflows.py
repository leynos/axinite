"""Reading workflows strictly, in every shape GitHub accepts.

Two readers the coverage-publication contract depends on and that nothing
else in this directory provides. `parse_strict` refuses a mapping that
declares a key twice, since PyYAML would keep the last value silently and a
job could carry a label in the discarded half. `triggers` reads `on:` the way
GitHub does: a bare event, a list, or a mapping, under the string key or the
boolean `True` PyYAML makes of an unquoted `on:`, and it refuses both at
once.

Split from `_coverage_publication.py` so neither module outgrows the
400-line limit; the policy there is what uses these.
"""

from __future__ import annotations

import typing as typ

import yaml

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path

#: Every parsed workflow, keyed by file name.
Workflows = dict[str, dict[object, object]]


class WorkflowReadError(ValueError):
    """A workflow could not be read in a shape these contracts can judge."""


class _StrictLoader(yaml.SafeLoader):
    """A SafeLoader that refuses a mapping declaring one key twice.

    PyYAML keeps the last value and says nothing, so a job declaring
    `runs-on` twice would be judged on one value while the file said two.
    """


def _construct_mapping(
    loader: _StrictLoader, node: yaml.MappingNode, deep: bool = False
) -> dict[object, object]:
    """Build a mapping, refusing a key that appears twice in it."""
    seen: set[object] = set()
    for key_node, _ in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in seen:
            message = f"duplicate key {key!r} at line {key_node.start_mark.line + 1}"
            raise WorkflowReadError(message)
        seen.add(key)
    return loader.construct_mapping(node, deep=deep)


_StrictLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG, _construct_mapping
)


def parse_strict(text: str, name: str) -> dict[object, object]:
    """Parse one workflow, refusing duplicate keys and a non-mapping root.

    Parameters
    ----------
    text
        The workflow's YAML source.
    name
        Its file name, quoted in a failure.

    Returns
    -------
    dict
        The parsed document.

    Raises
    ------
    WorkflowReadError
        If a mapping declares a key twice, the text is not YAML, or the root
        is not a mapping.
    """
    try:
        document = yaml.load(text, Loader=_StrictLoader)  # noqa: S506 - strict SafeLoader
    except WorkflowReadError as error:
        raise WorkflowReadError(f"{name}: {error}") from error
    except yaml.YAMLError as error:
        raise WorkflowReadError(f"{name}: not valid YAML ({error})") from error
    if not isinstance(document, dict):
        raise WorkflowReadError(f"{name}: the root is not a mapping")
    return document


def read_workflows(directory: Path) -> Workflows:
    """Read every workflow in a directory through the strict loader.

    Parameters
    ----------
    directory
        The workflow directory.

    Returns
    -------
    dict
        Each parsed workflow, keyed by file name.

    Raises
    ------
    WorkflowReadError
        If any workflow cannot be parsed strictly.
    """
    return {
        path.name: parse_strict(path.read_text(encoding="utf-8"), path.name)
        for path in sorted(directory.iterdir())
        if path.suffix.lower() in {".yml", ".yaml"} and path.is_file()
    }


def triggers(document: Mapping[object, object]) -> dict[str, object]:
    """Return a workflow's events, each with its configuration.

    Read as GitHub reads `on:`: a bare event name, a list of names, or a
    mapping of name to configuration, under the string key `on` or the
    boolean `True` PyYAML makes of an unquoted `on:`.

    Parameters
    ----------
    document
        A parsed workflow.

    Returns
    -------
    dict
        Event name to its configuration, `None` where it has none. Empty when
        the workflow declares no trigger at all.

    Raises
    ------
    WorkflowReadError
        If both spellings of the key are present, since GitHub merges them and
        a reader taking one would miss the other's events, or if the value is
        in a shape GitHub does not accept.
    """
    keys = [key for key in ("on", True) if key in document]
    if len(keys) > 1:
        raise WorkflowReadError("declares both `on` and an unquoted `on:`")
    return _events(document[keys[0]]) if keys else {}


def _events(declared: object) -> dict[str, object]:
    """Return the events one `on:` value declares, in any accepted shape."""
    match declared:
        case str():
            return {declared: None}
        case list() if all(isinstance(event, str) for event in declared):
            return dict.fromkeys(typ.cast("list[str]", declared))
        case dict() if all(isinstance(event, str) for event in declared):
            return {str(event): value for event, value in declared.items()}
        case _:
            raise WorkflowReadError(f"an `on:` of unmodelled shape {declared!r}")


def jobs(document: Mapping[object, object]) -> dict[str, dict[object, object]]:
    """Return a workflow's job mappings, keyed by job ID."""
    declared = document.get("jobs")
    if not isinstance(declared, dict):
        return {}
    return {str(key): job for key, job in declared.items() if isinstance(job, dict)}


def steps(job: Mapping[object, object]) -> list[dict[object, object]]:
    """Return a job's step mappings, in order."""
    declared = job.get("steps")
    if not isinstance(declared, list):
        return []
    return [step for step in declared if isinstance(step, dict)]
