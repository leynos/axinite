"""Contracts placing the `codescene` environment on the uploading job alone.

The estate test holds this repository's workflows to the placement. The unit
cases break each clause in a constructed workflow and drive the same query,
because the estate is correct and could not show a clause refusing anything.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import textwrap

import pytest
from _codescene_environment import environment_faults, environment_name
from _strict_workflows import Workflows, parse_strict, read_workflows
from _workflow_policy import WORKFLOW_DIR

#: A job calling the uploader, indented under `jobs:`.
UPLOADING_JOB = """\
  publish:
    runs-on: ubuntu-latest
{environment}    steps:
      - uses: leynos/shared-actions/.github/actions/upload-codescene-coverage@a
"""

#: A job that uploads nothing, indented under `jobs:`.
PLAIN_JOB = """\
  build:
    runs-on: ubuntu-latest
{environment}    steps:
      - run: make test
"""

#: The two ways GitHub accepts the environment, as a job-level line.
AS_NAME = "    environment: codescene\n"
AS_MAPPING = "    environment:\n      name: codescene\n"


def _estate(**sources: str) -> Workflows:
    """Parse workflows given as keyword arguments named for their files."""
    return {
        f"{name}.yml": parse_strict(textwrap.dedent(text), name)
        for name, text in sources.items()
    }


def _workflow(trigger: str, *job_blocks: str) -> str:
    """Return a workflow with one trigger and the given job blocks."""
    return f"on: {trigger}\njobs:\n" + "".join(job_blocks)


def test_the_estate_places_the_environment_on_the_uploader_alone() -> None:
    """Every uploading job enters `codescene`, and nothing else does."""
    faults = environment_faults(read_workflows(WORKFLOW_DIR))
    assert not faults, "\n".join(faults)


@pytest.mark.parametrize(
    "declaration",
    [pytest.param(AS_NAME, id="as-a-name"), pytest.param(AS_MAPPING, id="as-a-mapping")],
)
def test_an_uploader_in_the_environment_passes(declaration: str) -> None:
    """The narrow half: both accepted spellings satisfy the rule."""
    estate = _estate(
        publish=_workflow("push", UPLOADING_JOB.format(environment=declaration)),
        lane=_workflow("pull_request", PLAIN_JOB.format(environment="")),
    )
    faults = environment_faults(estate)
    assert faults == [], f"a correct placement should pass; it reported {faults}"


@pytest.mark.parametrize(
    ("estate", "fragment"),
    [
        pytest.param(
            {"publish": _workflow("push", UPLOADING_JOB.format(environment=""))},
            "publish.yml:publish uploads but does not declare",
            id="an-uploader-outside-the-environment",
        ),
        pytest.param(
            {
                "publish": _workflow("push", UPLOADING_JOB.format(environment=AS_NAME)),
                "other": _workflow("push", PLAIN_JOB.format(environment=AS_MAPPING)),
            },
            "other.yml:build declares `codescene` but uploads nothing",
            id="a-stray-job-in-the-environment",
        ),
        pytest.param(
            {"lane": _workflow("pull_request", UPLOADING_JOB.format(environment=AS_NAME))},
            "lane.yml:publish is reachable from a pull request",
            id="a-pull-request-job-in-the-environment",
        ),
        pytest.param(
            {"lane": _workflow("push", PLAIN_JOB.format(environment=""))},
            "no job calls the CodeScene uploader",
            id="no-uploader-at-all",
        ),
    ],
)
def test_each_misplacement_is_reported(estate: dict[str, str], fragment: str) -> None:
    """Each clause, broken alone, is reported by name.

    `a-pull-request-job-in-the-environment` uploads and declares, so only the
    surface clause can catch it: the branch policy would refuse the
    deployment, but the job should not be asking.
    """
    faults = environment_faults(_estate(**estate))
    assert any(fragment in fault for fault in faults), (
        f"expected a fault containing {fragment!r}; got {faults}"
    )


@pytest.mark.parametrize(
    ("job", "expected"),
    [
        pytest.param({"environment": "codescene"}, "codescene", id="a-name"),
        pytest.param(
            {"environment": {"name": "codescene", "url": "https://x"}},
            "codescene",
            id="a-mapping",
        ),
        pytest.param({"environment": {"url": "https://x"}}, None, id="no-name"),
        pytest.param({}, None, id="none"),
    ],
)
def test_the_environment_is_read_from_either_form(
    job: dict[object, object], expected: str | None
) -> None:
    """GitHub accepts a name or a `{name, url}` mapping; both read alike."""
    assert environment_name(job) == expected
