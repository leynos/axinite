"""Unit and property tests for the workflow-policy helpers.

The contracts in this directory are only as trustworthy as the parsing they
sit on. A helper that quietly returns nothing turns a policy assertion into a
vacuous pass: a job whose `runs-on` is written in a form the helper does not
read reports no labels, and every placement contract then waves it through.
These tests exercise the pure half of `_workflow_policy` directly, so each
supported shape is proven rather than assumed from whichever shapes the estate
happens to use today.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import pytest
from _workflow_policy import (
    SOURCE_BUILD_PATTERNS,
    UBICLOUD_LABEL,
    Job,
    builds_or_tests,
    cache_paths,
    declared_jobs_in,
    is_cache_step,
    jobs_of,
    parse_workflow,
    step_text,
    workflow_paths,
)
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: Hypothesis runs these against pure functions, but the suite shares a
#: machine with compiling CI jobs. A per-example deadline would turn host load
#: into a spurious failure, so only the example count is bounded.
PROPERTY = settings(
    deadline=None,
    max_examples=200,
    suppress_health_check=[HealthCheck.too_slow],
)

#: Runner labels wide enough to cover the estate's shapes without generating
#: YAML that no workflow could contain.
LABELS = st.text(
    alphabet="abcdefghijklmnopqrstuvwxyz0123456789-",
    min_size=1,
    max_size=12,
)


def _job(body: dict[str, object]) -> Job:
    """Wrap a job body for a helper under test."""
    return Job("test.yml", "example", body)


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

    def test_it_reports_the_job_as_workflow_and_id(self) -> None:
        """The string form identifies a job in assertion output."""
        assert str(_job({})) == "test.yml:example"


class TestRunnerLabels:
    """Every `runs-on` form the schema allows must be read, not skipped."""

    @pytest.mark.parametrize(
        ("declared", "expected"),
        [
            ("ubuntu-latest", ("ubuntu-latest",)),
            (["ubuntu-latest"], ("ubuntu-latest",)),
            (["self-hosted", "linux"], ("self-hosted", "linux")),
            ({"labels": "ubuntu-latest"}, ("ubuntu-latest",)),
            ({"labels": ["self-hosted", "linux"]}, ("self-hosted", "linux")),
            ({"group": "big", "labels": UBICLOUD_LABEL}, (UBICLOUD_LABEL,)),
            ({"group": "big"}, ()),
            (None, ()),
            ({}, ()),
            ([], ()),
            # A matrix expression is a string, so it reads as one label. The
            # placement contracts skip it deliberately; what matters here is
            # that it does not read as no labels at all.
            ("${{ matrix.runner }}", ("${{ matrix.runner }}",)),
        ],
        ids=[
            "scalar",
            "single-item-list",
            "multi-item-list",
            "mapping-scalar",
            "mapping-list",
            "mapping-group-and-label",
            "mapping-without-labels",
            "null",
            "empty-mapping",
            "empty-list",
            "matrix-expression",
        ],
    )
    def test_it_reads_each_supported_shape(
        self, declared: object, expected: tuple[str, ...]
    ) -> None:
        """Reading a shape as no labels would exempt the job from placement."""
        assert _job({"runs-on": declared}).runner_labels == expected

    @pytest.mark.parametrize(
        ("declared", "expected"),
        [
            (
                "${{ github.event_name == 'schedule' && 'ubuntu-latest' "
                "|| 'ubicloud-standard-8' }}",
                ("ubuntu-latest", "ubicloud-standard-8"),
            ),
            # A folded YAML scalar arrives with its line breaks already joined
            # into single spaces, which is how the workflows actually write it.
            (
                "${{ github.event_name == 'schedule' && 'ubuntu-latest'   "
                "|| 'ubicloud-standard-8' }}",
                ("ubuntu-latest", "ubicloud-standard-8"),
            ),
            (
                "${{ github.event_name == 'push' && 'ubicloud-standard-2' "
                "|| 'ubuntu-latest' }}",
                ("ubicloud-standard-2", "ubuntu-latest"),
            ),
        ],
        ids=["schedule", "folded-whitespace", "reversed"],
    )
    def test_an_event_conditional_runner_reports_both_arms(
        self, declared: str, expected: tuple[str, str]
    ) -> None:
        """Report both arms so a job that ever reaches Ubicloud is visible.

        Reading the expression as one opaque label would answer
        `uses_ubicloud` false and quietly drop the job from the placement,
        timeout, and sccache contracts at once.
        """
        job = _job({"runs-on": declared})
        assert job.runner_labels == expected
        assert job.uses_ubicloud is any(
            label.startswith("ubicloud-") for label in expected
        )

    @pytest.mark.parametrize(
        ("event", "expected"),
        [("schedule", ("ubuntu-latest",)), ("pull_request", ("ubicloud-standard-8",))],
        ids=["matching-event", "other-event"],
    )
    def test_labels_for_event_picks_the_arm_the_event_selects(
        self, event: str, expected: tuple[str, ...]
    ) -> None:
        """The scheduled-placement contract asks exactly this question."""
        job = _job(
            {
                "runs-on": "${{ github.event_name == 'schedule' && "
                "'ubuntu-latest' || 'ubicloud-standard-8' }}"
            }
        )
        assert job.labels_for_event(event) == expected

    @pytest.mark.parametrize(
        "declared",
        [
            "ubicloud-standard-8",
            "${{ matrix.runner }}",
            "${{ github.event_name == 'schedule' && 'a' }}",
            "${{ github.event_name == \"schedule\" && 'a' || 'b' }}",
        ],
        ids=["plain", "matrix", "no-else-arm", "double-quoted"],
    )
    def test_a_value_that_is_not_the_conditional_form_is_left_alone(
        self, declared: str
    ) -> None:
        """Only the exact shape is parsed; anything else stays one label."""
        job = _job({"runs-on": declared})
        assert job.runner_labels == (declared,)
        assert job.labels_for_event("schedule") == (declared,)

    def test_labels_for_event_matches_runner_labels_without_a_condition(
        self,
    ) -> None:
        """A job whose runner does not depend on the event answers the same."""
        job = _job({"runs-on": ["self-hosted", "linux"]})
        assert job.labels_for_event("schedule") == job.runner_labels

    def test_a_job_without_runs_on_declares_no_labels(self) -> None:
        """A reusable-workflow caller declares no runner of its own."""
        assert (
            _job(
                {"uses": "leynos/shared-actions/.github/workflows/x.yml@sha"}
            ).runner_labels
            == ()
        )

    def test_non_string_list_entries_are_dropped(self) -> None:
        """A malformed entry must not become a label."""
        assert _job({"runs-on": ["linux", 7, None]}).runner_labels == ("linux",)

    @pytest.mark.parametrize(
        ("declared", "expected"),
        [
            ("ubuntu-latest", "ubuntu-latest"),
            (["ubuntu-latest"], "ubuntu-latest"),
            (["self-hosted", "linux"], None),
            (None, None),
        ],
        ids=["scalar", "single", "several", "absent"],
    )
    def test_runs_on_answers_only_for_a_single_label(
        self, declared: object, expected: str | None
    ) -> None:
        """`runs_on` is the unambiguous case; several labels have no answer."""
        assert _job({"runs-on": declared}).runs_on == expected

    def test_the_runner_summary_names_every_label(self) -> None:
        """The summary is what an assertion message shows."""
        assert _job({"runs-on": ["a", "b"]}).runner_summary == "a, b"
        assert _job({}).runner_summary == "<none>"

    @pytest.mark.parametrize(
        ("declared", "ubicloud"),
        [
            (UBICLOUD_LABEL, True),
            ("ubicloud-standard-2", True),
            (["ubuntu-latest", "ubicloud-standard-4"], True),
            ("ubuntu-latest", False),
            ("windows-latest", False),
        ],
        ids=["current", "smaller", "mixed", "github-linux", "github-windows"],
    )
    def test_ubicloud_detection_matches_on_the_prefix(
        self, declared: object, *, ubicloud: bool
    ) -> None:
        """Keying on one exact label would wave a resized runner through."""
        job = _job({"runs-on": declared})
        assert job.uses_ubicloud is ubicloud
        assert bool(job.ubicloud_labels) is ubicloud


class TestSteps:
    """Step readers tolerate the forms a workflow may legally take."""

    def test_a_job_without_steps_reads_as_none(self) -> None:
        """A reusable-workflow caller declares no steps."""
        assert _job({"uses": "owner/repo/.github/workflows/x.yml@sha"}).steps == []

    def test_non_mapping_steps_are_dropped(self) -> None:
        """A malformed step must not reach a contract as a mapping."""
        assert _job({"steps": [{"run": "make"}, "oops", None]}).steps == [
            {"run": "make"}
        ]

    def test_step_text_is_empty_for_an_action_step(self) -> None:
        """An action step runs no shell, so it has no command to classify."""
        assert step_text({"uses": "actions/checkout@v6"}) == ""
        assert step_text({"run": "make test"}) == "make test"

    @pytest.mark.parametrize(
        ("step", "expected"),
        [
            (
                {"with": {"path": "~/.cargo/registry\n~/.cargo/git\n"}},
                ["~/.cargo/registry", "~/.cargo/git"],
            ),
            ({"with": {"path": "  ~/.cargo/registry  "}}, ["~/.cargo/registry"]),
            ({"with": {"path": "a\n\n\nb"}}, ["a", "b"]),
            ({"with": {"path": ["a", "b"]}}, []),
            ({"with": {}}, []),
            ({}, []),
        ],
        ids=["multiline", "padded", "blank-lines", "sequence", "no-path", "no-with"],
    )
    def test_cache_paths_reads_one_path_per_line(
        self, step: dict[str, object], expected: list[str]
    ) -> None:
        """Blank lines and padding are formatting, not cache entries."""
        assert cache_paths(step) == expected

    @pytest.mark.parametrize(
        ("uses", "expected"),
        [
            ("actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", True),
            ("actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", True),
            ("actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", True),
            ("actions/checkout@v6", False),
            ("Swatinem/rust-cache@v2", False),
        ],
        ids=["combined", "restore", "save", "checkout", "rust-cache"],
    )
    def test_cache_steps_cover_the_sub_actions(
        self, uses: str, *, expected: bool
    ) -> None:
        """Missing a sub-action would hide half of a cache's ownership."""
        assert is_cache_step({"uses": uses}) is expected

    def test_a_run_step_is_not_a_cache_step(self) -> None:
        """`is_cache_step` reads `uses`, which a run step does not have."""
        assert is_cache_step({"run": "actions/cache@v6"}) is False


class TestClassification:
    """A job's claim on a paid runner comes from what it runs."""

    @pytest.mark.parametrize(
        "command",
        [
            "cargo build --workspace",
            "cargo +nightly test",
            "cargo llvm-cov nextest --workspace",
            "cargo fmt --all -- --check",
            "docker build -t axinite-test:ci .",
            "make build-github-tool-wasm",
            "./scripts/build-wasm-extensions.sh --channels",
            "uv run pytest tests/workflow_contracts",
        ],
        ids=[
            "build",
            "toolchain-selector",
            "coverage",
            "fmt",
            "docker",
            "make-wasm",
            "wasm-script",
            "pytest",
        ],
    )
    def test_a_compiling_command_classifies_the_job(self, command: str) -> None:
        """A build or test command is what earns the runner."""
        assert builds_or_tests(_job({"steps": [{"run": command}]}))

    @pytest.mark.parametrize(
        "command",
        [
            "cargo audit --file Cargo.lock",
            "cargo binstall --no-confirm cargo-nextest@0.9.140",
            "gh pr view 345",
            "",
        ],
        ids=["audit", "binstall", "gh", "action-step"],
    )
    def test_metadata_commands_do_not_classify_the_job(self, command: str) -> None:
        """Reading metadata or downloading an archive compiles nothing."""
        assert not builds_or_tests(_job({"steps": [{"run": command}]}))


class TestSourceBuildPatterns:
    """Fail-closed installs are the point; a strategy list can still lie."""

    @staticmethod
    def _reasons(command: str) -> list[str]:
        """Return the reasons a command counts as a source build."""
        return [
            reason
            for pattern, reason in SOURCE_BUILD_PATTERNS
            if pattern.search(command)
        ]

    @pytest.mark.parametrize(
        "command",
        [
            "cargo install cargo-nextest",
            "cargo +nightly install cargo-component",
            "cargo binstall --no-confirm cargo-component@0.21.1",
            "cargo binstall --strategies compile cargo-component@0.21.1",
            "cargo binstall --strategies crate-meta-data,compile "
            "cargo-component@0.21.1",
        ],
        ids=[
            "install",
            "install-with-toolchain",
            "binstall-without-strategies",
            "compile-only",
            "compile-in-a-list",
        ],
    )
    def test_a_source_build_is_reported(self, command: str) -> None:
        """Naming `compile` asks for the build the strategy list exists to stop."""
        assert self._reasons(command)

    @pytest.mark.parametrize(
        "command",
        [
            "cargo binstall --no-confirm --strategies crate-meta-data,quick-install "
            '"cargo-component@$CARGO_COMPONENT_PIN"',
            "cargo binstall --strategies quick-install cargo-nextest@0.9.140",
            "cargo build --workspace",
        ],
        ids=["fail-closed", "quick-install", "not-an-install"],
    )
    def test_a_fail_closed_install_is_not_reported(self, command: str) -> None:
        """The proven installer form must stay clean or the contract is noise."""
        assert not self._reasons(command)


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


@given(labels=st.lists(LABELS, min_size=0, max_size=4))
@PROPERTY
def test_equivalent_runs_on_forms_read_the_same_labels(labels: list[str]) -> None:
    """The three ways to write the same runner must not disagree.

    A shape that read differently from its equivalent would let an author
    move a job off the placement contracts by rewriting its `runs-on`.
    """
    expected = tuple(labels)
    assert _job({"runs-on": labels}).runner_labels == expected
    assert _job({"runs-on": {"labels": labels}}).runner_labels == expected
    if len(labels) == 1:
        assert _job({"runs-on": labels[0]}).runner_labels == expected
        assert _job({"runs-on": {"labels": labels[0]}}).runner_labels == expected


@given(labels=st.lists(LABELS, max_size=4))
@PROPERTY
def test_runs_on_answers_exactly_when_one_label_is_declared(
    labels: list[str],
) -> None:
    """`runs_on` must be None whenever the label is ambiguous."""
    job = _job({"runs-on": labels})
    assert (job.runs_on is not None) is (len(labels) == 1)
    if job.runs_on is not None:
        assert job.runs_on == labels[0]


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


@given(
    lines=st.lists(
        st.one_of(st.just(""), st.just("   "), st.text(alphabet="ab/~.", max_size=8)),
        max_size=6,
    )
)
@PROPERTY
def test_cache_paths_never_yields_an_empty_entry(lines: list[str]) -> None:
    """An empty path would read as a cache owner claiming nothing."""
    found = cache_paths({"with": {"path": "\n".join(lines)}})
    assert all(path == path.strip() and path for path in found)
    assert found == [line.strip() for line in lines if line.strip()]
