"""Unit and property tests for reading workflow text into documents and jobs.

Split from `workflow_policy_helpers_test.py` with the module they test,
`_workflow_files.py`. A reader that dropped a job, or half-read a malformed
one, would leave every contract over the estate passing on less than the
estate declares.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _workflow_files import declared_jobs_in, jobs_of, parse_workflow, workflow_paths
from _workflow_policy import Job
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: Hypothesis runs these against pure functions, but the suite shares a
#: machine with compiling CI jobs, so only the example count is bounded.
PROPERTY = settings(
    deadline=None,
    max_examples=200,
    suppress_health_check=[HealthCheck.too_slow],
)

#: Runner labels wide enough to cover the estate's shapes.
LABELS = st.text(
    alphabet="abcdefghijklmnopqrstuvwxyz0123456789-",
    min_size=1,
    max_size=12,
)


class TestParseWorkflow:
    """`parse_workflow` accepts a workflow and rejects anything else."""

    def test_it_returns_the_parsed_mapping(self) -> None:
        """A workflow document parses to its mapping."""
        assert parse_workflow("name: Example\njobs: {}\n", "x.yml") == {
            "name": "Example",
            "jobs": {},
        }

    @pytest.mark.parametrize(
        "text",
        ["", "- one\n- two\n", "just a string\n", "null\n"],
        ids=["empty", "sequence", "scalar", "null"],
    )
    def test_it_rejects_a_document_that_is_not_a_mapping(self, text: str) -> None:
        """A non-mapping document is not a workflow and must fail loudly."""
        with pytest.raises(AssertionError, match=r"x\.yml must parse as a mapping"):
            parse_workflow(text, "x.yml")


class TestDeclaredJobs:
    """`declared_jobs_in` and `jobs_of` read only well-formed jobs."""

    @pytest.mark.parametrize(
        "document",
        [{}, {"jobs": None}, {"jobs": []}, {"jobs": "build"}],
        ids=["absent", "null", "sequence", "scalar"],
    )
    def test_a_missing_or_malformed_jobs_mapping_reads_as_empty(
        self, document: dict[str, object]
    ) -> None:
        """Only a mapping counts as a jobs declaration."""
        assert declared_jobs_in(document) == {}

    def test_it_skips_a_job_whose_body_is_not_a_mapping(self) -> None:
        """A malformed job body yields no Job rather than raising."""
        document = {"jobs": {"good": {"runs-on": "ubuntu-latest"}, "bad": None}}
        found = list(jobs_of("test.yml", document))
        assert [job.job_id for job in found] == ["good"]
        assert found[0].workflow == "test.yml"


class TestWorkflowPaths:
    """The scan is the file-reading edge, and it reads only workflows."""

    def test_it_returns_both_workflow_extensions_in_name_order(
        self, tmp_path: Path
    ) -> None:
        """GitHub accepts `.yaml` too, and a stable order keeps test ids stable.

        Scanning one extension would exempt a `.yaml` workflow from the
        runner, timeout, cache, and tool-install contracts at once, with every
        test still passing.
        """
        for name in ("test.yml", "audit.yml", "notes.md", "release.yaml", "a.txt"):
            (tmp_path / name).write_text("{}\n", encoding="utf-8")
        (tmp_path / "nested.yml").mkdir()
        assert [path.name for path in workflow_paths(tmp_path)] == [
            "audit.yml",
            "release.yaml",
            "test.yml",
        ]

    def test_an_empty_directory_yields_nothing(self, tmp_path: Path) -> None:
        """An empty scan must not raise."""
        assert workflow_paths(tmp_path) == []


@given(
    bodies=st.dictionaries(
        st.text(alphabet="abcdefgh", min_size=1, max_size=4),
        st.one_of(
            st.dictionaries(st.just("runs-on"), LABELS, max_size=1),
            st.none(),
            st.text(max_size=4),
            st.lists(st.integers(), max_size=2),
        ),
        max_size=6,
    )
)
@PROPERTY
def test_only_mapping_job_bodies_become_jobs(
    bodies: dict[str, object],
) -> None:
    """A malformed job body is skipped, never half-read."""
    found = list(jobs_of("test.yml", {"jobs": bodies}))
    assert [job.job_id for job in found] == [
        job_id for job_id, body in bodies.items() if isinstance(body, dict)
    ]
    assert all(job.workflow == "test.yml" for job in found)


def test_a_job_reports_itself_as_workflow_and_id() -> None:
    """The string form identifies a job read from a file in assertion output."""
    assert str(Job("test.yml", "example", {})) == "test.yml:example"
