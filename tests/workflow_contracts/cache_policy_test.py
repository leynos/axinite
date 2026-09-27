"""Unit tests for the cache-step and push-filter readings.

`cache_ownership_test.py` applies these readings to the estate, whose two
registry writers are correct and triggered on `main`; a contract
parametrized over correct input discriminates nothing. The cases here state
each shape the readings must refuse, and the shapes they must accept, against
constructed jobs and triggers.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _cache_policy import (
    COMBINED_ACTION,
    RESTORE_ACTION,
    SAVE_ACTION,
    RegistryEntry,
    cache_paths,
    cache_steps,
    is_cache_step,
    registry_entries,
    writer_job_faults,
)
from _push_filters import PushFilterError, glob_matches, push_reaches_branch
from _workflow_policy import Job
from hypothesis import HealthCheck, given, settings
from hypothesis import strategies as st

#: A registry key as the estate writes it.
REGISTRY_KEY = "cargo-v1-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}"
#: The paths the estate archives under that key.
REGISTRY_PATHS = "~/.cargo/registry\n~/.cargo/git\n"
#: A trigger set that reaches `main`.
PUSH_TO_MAIN: dict[object, object] = {"push": {"branches": ["main"]}}


def _cache_job(uses: str, *, path: str = REGISTRY_PATHS, key: str = REGISTRY_KEY) -> Job:
    """Return a Linux job with one cache step."""
    step = {"uses": f"{uses}sha", "with": {"key": key, "path": path}}
    return Job("ci.yml", "build", {"runs-on": "ubuntu-latest", "steps": [step]})


@pytest.mark.parametrize(
    ("declared", "expected"),
    [
        pytest.param({}, False, id="no-triggers-at-all"),
        pytest.param({"pull_request": None}, False, id="pull-request-only"),
        pytest.param({"push": None}, True, id="a-bare-push-accepts-every-branch"),
        pytest.param({"push": {}}, True, id="a-push-without-filters"),
        pytest.param({"push": {"branches": ["main"]}}, True, id="main-named"),
        pytest.param({"push": {"branches": "main"}}, True, id="main-as-a-string"),
        pytest.param(
            {"push": {"branches": ["main", "release"]}}, True, id="main-among-others"
        ),
        pytest.param({"push": {"branches": ["release"]}}, False, id="main-left-out"),
        pytest.param({"push": {"branches": ["*"]}}, True, id="a-wildcard"),
        pytest.param({"push": {"branches": ["ma*"]}}, True, id="a-prefix-glob"),
        pytest.param(
            {"push": {"branches": ["release/**"]}}, False, id="a-glob-elsewhere"
        ),
        pytest.param(
            {"push": {"branches": ["**", "!main"]}}, False, id="negated-after"
        ),
        pytest.param(
            {"push": {"branches": ["**", "!main", "main"]}},
            True,
            id="re-admitted-after-negation",
        ),
        pytest.param(
            {"push": {"branches-ignore": ["main"]}}, False, id="main-ignored"
        ),
        pytest.param(
            {"push": {"branches-ignore": ["ma?in"]}}, False, id="main-ignored-by-glob"
        ),
        pytest.param(
            {"push": {"branches-ignore": ["release"]}}, True, id="another-ignored"
        ),
        pytest.param({"push": {"tags": ["v*"]}}, False, id="tags-only"),
        pytest.param({"push": {"tags-ignore": ["v*"]}}, False, id="tags-ignore-only"),
        pytest.param(
            {"push": {"tags": ["v*"], "branches": ["main"]}},
            True,
            id="tags-and-main",
        ),
    ],
)
def test_a_push_filter_is_read_as_github_reads_it(
    declared: dict[object, object], *, expected: bool
) -> None:
    """Each filter shape admits `main` exactly when GitHub would.

    The pair that started this is `a-bare-push-accepts-every-branch` against
    `pull-request-only`: both return `None` from `.get("push")` and they mean
    opposite things. `main-ignored`, `negated-after` and `tags-only` all hold
    no `main` in a `branches` list, and all three refuse a push to `main`.
    """
    assert push_reaches_branch(declared, "main") is expected, (
        f"{declared!r} should read as reaches-main={expected}"
    )


@pytest.mark.parametrize(
    "push",
    [
        pytest.param({"branches": ["main"], "branches-ignore": ["x"]}, id="both"),
        pytest.param({"branches": ["+main"]}, id="an-uncompilable-pattern"),
        pytest.param({"branches": [1]}, id="a-non-string-pattern"),
        pytest.param(["main"], id="a-list-for-a-mapping"),
    ],
)
def test_a_push_filter_this_cannot_read_is_refused(push: object) -> None:
    """Guessing either way could pass a writer that never runs."""
    with pytest.raises(PushFilterError):
        push_reaches_branch({"push": push}, "main")


@pytest.mark.parametrize(
    ("pattern", "ref", "expected"),
    [
        pytest.param("feature/*", "feature/a", True, id="star-in-one-segment"),
        pytest.param("feature/*", "feature/a/b", False, id="star-stops-at-slash"),
        pytest.param("feature/**", "feature/a/b", True, id="double-star-crosses"),
        pytest.param("v1.0", "v1x0", False, id="a-dot-is-literal"),
        pytest.param("ma+in", "maaain", True, id="plus-repeats"),
        pytest.param("mai?n", "man", True, id="question-is-optional"),
        pytest.param("[mr]ain", "main", True, id="a-class"),
        pytest.param("main", "main-2", False, id="whole-name-only"),
    ],
)
def test_a_glob_follows_githubs_grammar(pattern: str, ref: str, *, expected: bool) -> None:
    """`*`, `**`, `?`, `+` and classes mean what GitHub says they mean."""
    assert glob_matches(pattern, ref) is expected


@pytest.mark.parametrize(
    ("condition", "declared_on", "expected"),
    [
        pytest.param(None, PUSH_TO_MAIN, [], id="unguarded-on-main"),
        pytest.param(
            "github.event_name == 'push'", PUSH_TO_MAIN, [], id="guarded-on-push"
        ),
        pytest.param(
            "github.event_name != 'push'",
            PUSH_TO_MAIN,
            ["its own condition refuses the push event"],
            id="guard-refuses-push",
        ),
        pytest.param(
            "github.ref != 'refs/heads/main'",
            PUSH_TO_MAIN,
            ["constrains the ref"],
            id="guard-refuses-main",
        ),
        pytest.param(
            "github.ref_name == 'release'",
            PUSH_TO_MAIN,
            ["constrains the ref"],
            id="guard-names-another-branch",
        ),
        pytest.param(
            None,
            {"push": {"branches-ignore": ["main"]}},
            ["not triggered by a push to main"],
            id="workflow-ignores-main",
        ),
    ],
)
def test_a_writer_job_must_run_on_a_push_to_main(
    condition: str | None, declared_on: dict[object, object], expected: list[str]
) -> None:
    """The workflow trigger, the event guard and the ref guard all count.

    `guard-refuses-main` is the case `runs_on_event` alone accepted: the
    condition never names an event, so it reads as runnable on a push.
    """
    body: dict[str, object] = {"runs-on": "ubuntu-latest"}
    if condition is not None:
        body["if"] = condition
    faults = writer_job_faults(Job("ci.yml", "build", body), declared_on)
    assert len(faults) == len(expected), f"expected {expected}; got {faults}"
    for fault, fragment in zip(faults, expected, strict=True):
        assert fragment in fault, f"{fault!r} should say {fragment!r}"


def test_the_three_cache_entry_points_do_not_overlap() -> None:
    """The combined action is found as itself and never as a sub-action."""
    jobs = [_cache_job(COMBINED_ACTION), _cache_job(SAVE_ACTION)]
    assert [job for job, _ in cache_steps(jobs, COMBINED_ACTION)] == [jobs[0]]
    assert [job for job, _ in cache_steps(jobs, SAVE_ACTION)] == [jobs[1]]
    assert list(cache_steps(jobs, RESTORE_ACTION)) == []


@pytest.mark.parametrize(
    ("saved", "matches"),
    [
        pytest.param({}, True, id="same-key-and-paths"),
        pytest.param({"path": "~/.cargo/registry\n"}, False, id="fewer-paths"),
        pytest.param(
            {"key": "cargo-v1-${{ runner.os }}-other"}, False, id="another-key"
        ),
    ],
)
def test_a_writer_must_save_what_the_reader_restores(
    saved: dict[str, str], *, matches: bool
) -> None:
    """Sharing the key prefix is not enough; key and paths must both agree.

    GitHub matches a cache on its key and on a version derived from the
    paths, so either difference leaves the restore unfilled.
    """
    restored = registry_entries([_cache_job(RESTORE_ACTION)], RESTORE_ACTION)
    written = registry_entries([_cache_job(SAVE_ACTION, **saved)], SAVE_ACTION)
    assert (restored <= written) is matches, f"{restored} against {written}"


def test_a_registry_entry_names_platform_key_and_paths() -> None:
    """The positive half: the entry is what the step says."""
    (entry,) = registry_entries([_cache_job(RESTORE_ACTION)], RESTORE_ACTION)
    assert entry == RegistryEntry(
        "Linux", REGISTRY_KEY, frozenset({"~/.cargo/registry", "~/.cargo/git"})
    )


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
    step: dict[object, object], expected: list[str]
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
def test_cache_steps_cover_the_sub_actions(uses: str, *, expected: bool) -> None:
    """Missing a sub-action would hide half of a cache's ownership."""
    assert is_cache_step({"uses": uses}) is expected


def test_a_run_step_is_not_a_cache_step() -> None:
    """`is_cache_step` reads `uses`, which a run step does not have."""
    assert is_cache_step({"run": "actions/cache@v6"}) is False


@given(
    lines=st.lists(
        st.one_of(st.just(""), st.just("   "), st.text(alphabet="ab/~.", max_size=8)),
        max_size=6,
    )
)
@settings(deadline=None, max_examples=200, suppress_health_check=[HealthCheck.too_slow])
def test_cache_paths_never_yields_an_empty_entry(lines: list[str]) -> None:
    """An empty path would read as a cache owner claiming nothing."""
    found = cache_paths({"with": {"path": "\n".join(lines)}})
    assert all(path == path.strip() and path for path in found)
    assert found == [line.strip() for line in lines if line.strip()]
