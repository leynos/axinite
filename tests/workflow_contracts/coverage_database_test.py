"""Contracts binding the coverage job's Postgres legs to the embedded cluster.

`src/testing/postgres.rs` takes its database from `TEST_DATABASE_URL` when one
is named, and otherwise, with the `embedded-postgres` feature on Linux, from an
embedded cluster the test process owns. The coverage job used to start a
Postgres service container and name it through `TEST_DATABASE_URL`; it now runs
every Postgres-bearing leg on the embedded cluster, so the contracts here are
the reverse of the old ones.

The failure to keep out is quiet. A leg that compiles `postgres` but not
`embedded-postgres`, and exports no URL, has no database source, and its
Postgres tests skip, so the leg reports coverage for tests that never ran. A
leg that still exports a URL would bypass the cluster. See issue #350 for the
history of this job's database wiring.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import shlex
import typing as typ

import pytest
import tomllib
from _workflow_files import load
from _workflow_policy import REPOSITORY_ROOT, WORKFLOW_DIR, step_text

WORKFLOW: typ.Final[str] = "coverage.yml"

#: The variable `src/testing/postgres.rs` reads. Renaming the export without
#: renaming the reader is the exact mistake this file exists to catch, so the
#: constant is spelled out here rather than derived from the workflow.
TEST_URL_VARIABLE: typ.Final[str] = "TEST_DATABASE_URL"

#: The job that runs the Postgres-bearing coverage legs.
JOB: typ.Final[str] = "coverage"

def _postgres_job() -> dict[str, object]:
    """Return the coverage job, failing loudly if it is renamed away."""
    jobs = load(WORKFLOW_DIR / WORKFLOW).get("jobs")
    assert isinstance(jobs, dict), f"{WORKFLOW} must declare a jobs mapping"
    job = jobs.get(JOB)
    assert isinstance(job, dict), f"{WORKFLOW} must declare a {JOB!r} job"
    return job


def _steps(job: dict[str, object]) -> list[dict[str, object]]:
    """Return the job's step mappings."""
    steps = job.get("steps")
    assert isinstance(steps, list), f"{JOB} must declare steps"
    return [step for step in steps if isinstance(step, dict)]


def test_the_coverage_job_starts_no_database_service() -> None:
    """The coverage job provisions its database through pg-embed, not a service.

    A service container is a second source of truth for the database and the
    reason this job needed a migration step and a credentialed URL. Neither
    exists any more, and a service that crept back would start for every leg,
    `libsql-only` included.
    """
    assert "services" not in _postgres_job(), (
        f"{JOB} must not declare a services mapping: its Postgres legs run on "
        "the embedded cluster"
    )


def test_the_coverage_job_names_no_database() -> None:
    """No step may export a database URL, which would take precedence.

    A named `TEST_DATABASE_URL` is used before the embedded cluster, so a leg
    that exported one would silently skip the cluster it is meant to run on.
    `DATABASE_URL` is the runtime's own name for the same thing and is held to
    the same rule.
    """
    for step in _steps(_postgres_job()):
        script = step_text(step)
        for variable in (TEST_URL_VARIABLE, "DATABASE_URL"):
            assert f"{variable}=" not in script, (
                f"the step {step.get('name')!r} exports {variable}; the "
                "Postgres legs must use the embedded cluster"
            )


def test_every_postgres_bearing_leg_enables_the_embedded_cluster() -> None:
    """A leg compiles `embedded-postgres` exactly when it bears Postgres.

    `--all-features` carries it implicitly. The narrow direction keeps the
    libSQL-only leg light: the feature implies `postgres`, so naming it there
    would give that leg Postgres after all.
    """
    for leg in _legs():
        flags = str(leg.get("flags", ""))
        tokens = shlex.split(flags, comments=False, posix=True)
        named: set[str] = set()
        for index, token in enumerate(tokens):
            following = tokens[index + 1] if index + 1 < len(tokens) else None
            named |= _features_named_by(token, following)
        embedded = "--all-features" in tokens or EMBEDDED_FEATURE in named
        declared = bool(leg.get("has_postgres"))
        assert embedded == declared, (
            f"the {leg.get('name')!r} leg declares has_postgres={declared} but "
            f"its flags {flags!r} resolve to {EMBEDDED_FEATURE}={embedded}; a "
            "Postgres-bearing leg without the embedded cluster skips its "
            "database tests"
        )


#: The variable a lane sets to say that Postgres is not optional on this run.
#: `src/testing/postgres.rs` skips its Postgres tests when the database is
#: unreachable, which is what a developer without one needs. On a lane that
#: starts a Postgres service and points the tests at it, the same skip reports
#: success for tests that never connected.
REQUIRE_VARIABLE: typ.Final[str] = "AXINITE_REQUIRE_POSTGRES"

#: The feature that gives a leg its embedded cluster.
EMBEDDED_FEATURE: typ.Final[str] = "embedded-postgres"

#: The feature whose presence makes a leg Postgres-bearing.
POSTGRES_FEATURE: typ.Final[str] = "postgres"

#: The guard a step carries to run only on the Postgres-bearing legs.
POSTGRES_GUARD: typ.Final[str] = "matrix.has_postgres"


def _default_features() -> frozenset[str]:
    """Return the root package's default feature set.

    Read rather than restated, because `postgres` being a default feature is
    the whole reason a leg can bear Postgres without naming it.
    """
    manifest = tomllib.loads(
        (REPOSITORY_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    )
    declared = manifest.get("features", {}).get("default", [])
    return frozenset(str(name) for name in declared)


#: `echo "NAME=value" >> "$GITHUB_ENV"`, anchored so the redirection names
#: that file and not a variable whose name merely starts the same way.
#: `$GITHUB_ENV_UNUSED`, `$GITHUB_ENVIRONMENT` and `$GITHUB_ENV.backup` are all
#: plausible typos, none of them reaches a later step, and a pattern that
#: stopped at `GITHUB_ENV` would accept every one while the tests silently
#: skipped.
REQUIRE_APPEND_RE: typ.Final[re.Pattern[str]] = re.compile(
    rf'echo\s+"{REQUIRE_VARIABLE}=[^"\n]+"\s*>>\s*'
    r'(?:"\$\{GITHUB_ENV\}"|"\$GITHUB_ENV"|\$\{GITHUB_ENV\}|\$GITHUB_ENV)'
    r"\s*$",
    re.MULTILINE,
)

#: How Cargo spells a feature list. The long and short flags each take their
#: value separately or joined with `=`, and the value separates on commas or
#: whitespace: `--features "libsql postgres"` is one argument naming two
#: features. Reading one spelling and calling the others empty would report a
#: Postgres-bearing leg as narrow.
FEATURE_FLAGS: typ.Final[tuple[str, ...]] = ("--features", "-F")
FEATURE_SEPARATORS: typ.Final[re.Pattern[str]] = re.compile(r"[,\s]+")


def _split_features(value: str) -> set[str]:
    """Return the feature names one `--features` value carries."""
    return {name for name in FEATURE_SEPARATORS.split(value.strip()) if name}


def _is_short_flag_with_a_joined_value(token: str) -> bool:
    """Report whether a token is `-F` carrying its value without a separator.

    `-Flibsql` is the short flag with its value joined on, which clap accepts.
    The long flag has no such form, so only the short one is read this way:
    treating `--featuresx` as a feature list would invent one. `-F=libsql` is
    the separated form and is read before this.

    Parameters
    ----------
    token
        One shell word of the command.

    Returns
    -------
    bool
        True when the token is the short flag with a joined, non-empty value.
    """
    if not token.startswith("-F"):
        return False
    value = token[2:]
    return bool(value) and not value.startswith("=")


def _features_named_by(token: str, following: str | None) -> set[str]:
    """Return the features one argument names.

    Parameters
    ----------
    token
        One shell word of the command.
    following
        The word after it, when there is one. The separated forms take their
        value there.

    Returns
    -------
    set of str
        The feature names, empty for every argument that names none.
    """
    if token in FEATURE_FLAGS:
        return _split_features(following) if following is not None else set()
    for flag in FEATURE_FLAGS:
        if token.startswith(f"{flag}="):
            return _split_features(token[len(flag) + 1 :])
    if _is_short_flag_with_a_joined_value(token):
        return _split_features(token[2:])
    return set()


def _enables_postgres(flags: str) -> bool:
    """Report whether a leg's flags compile the `postgres` feature.

    Parameters
    ----------
    flags
        The leg's `flags` value, as handed to `cargo llvm-cov nextest`.

    Returns
    -------
    bool
        True when the resolved feature set contains `postgres`.
    """
    tokens = shlex.split(flags, comments=False, posix=True)
    if "--all-features" in tokens:
        return True
    named: set[str] = set()
    for index, token in enumerate(tokens):
        following = tokens[index + 1] if index + 1 < len(tokens) else None
        named.update(_features_named_by(token, following))
    # Cargo enables the defaults unless the command turns them off, so a leg
    # that names nothing still gets every member of `default`.
    if "--no-default-features" not in tokens:
        named |= _default_features()
    return POSTGRES_FEATURE in named


def _legs() -> list[dict[str, object]]:
    """Return the coverage job's matrix legs."""
    strategy = _postgres_job().get("strategy")
    assert isinstance(strategy, dict), f"{JOB} must declare a strategy"
    matrix = strategy.get("matrix")
    assert isinstance(matrix, dict), f"{JOB} must declare a matrix"
    include = matrix.get("include")
    assert isinstance(include, list), f"{JOB}'s matrix must declare include"
    return [leg for leg in include if isinstance(leg, dict)]


def test_every_postgres_bearing_leg_declares_it() -> None:
    """A leg's `has_postgres` must match the features it actually compiles.

    The flag decides whether the service is used, the migrations run and the
    database URL is exported. A leg that compiles `postgres` without the flag
    runs every Postgres test against nothing and skips them all, and one that
    carries the flag without the feature pays for a service it cannot use.
    """
    for leg in _legs():
        name = leg.get("name")
        declared = bool(leg.get("has_postgres"))
        actual = _enables_postgres(str(leg.get("flags", "")))
        assert declared == actual, (
            f"the {name!r} leg declares has_postgres={declared} but its flags "
            f"{leg.get('flags')!r} resolve to postgres={actual}; `postgres` is "
            "a default feature, so a leg gets it unless it passes "
            "--no-default-features"
        )


def test_the_postgres_legs_tell_the_tests_the_database_is_not_optional() -> None:
    """The promise of a database reaches the step that runs the tests.

    Without it a leg whose cluster cannot be had reports success for every test
    that needed it. It is appended to `$GITHUB_ENV`: setting it in the step's
    own shell reaches nothing that runs the tests.
    """
    steps = [
        step
        for step in _steps(_postgres_job())
        if f"{REQUIRE_VARIABLE}=" in step_text(step)
    ]
    assert steps, f"{JOB} must export {REQUIRE_VARIABLE} for its Postgres legs"
    assert REQUIRE_APPEND_RE.search(step_text(steps[0])), (
        f"{REQUIRE_VARIABLE} must be appended to $GITHUB_ENV; setting it in "
        "the step's own shell reaches nothing that runs the tests"
    )


def test_the_promise_is_confined_to_the_postgres_bearing_legs() -> None:
    """A leg with no database must keep its skip.

    The narrow direction. Exporting the requirement unconditionally would fail
    the libsql-only leg, which is meant to run without a database, and a
    developer's checkout inherits nothing from here either way.
    """
    exporting = [
        step
        for step in _steps(_postgres_job())
        if f"{REQUIRE_VARIABLE}=" in step_text(step)
    ]
    assert len(exporting) == 1, (
        f"expected exactly one step exporting {REQUIRE_VARIABLE}, found "
        f"{len(exporting)}"
    )
    condition = " ".join(str(exporting[0].get("if", "")).split())
    assert condition == POSTGRES_GUARD, (
        f"the step exporting {REQUIRE_VARIABLE} is guarded by {condition!r}, "
        f"not {POSTGRES_GUARD!r}; a leg without a database would be told that "
        "one is mandatory and fail"
    )


@pytest.mark.parametrize(
    ("flags", "expected"),
    [
        pytest.param("--all-features", True, id="all-features"),
        pytest.param("", True, id="nothing-named-gets-the-defaults"),
        pytest.param("--features libsql", True, id="defaults-plus-a-name"),
        pytest.param(
            "--no-default-features --features libsql", False, id="narrow-and-named"
        ),
        pytest.param(
            "--no-default-features --features postgres", True, id="narrow-but-postgres"
        ),
        pytest.param(
            "--no-default-features --features libsql,postgres",
            True,
            id="comma-separated",
        ),
        pytest.param(
            "--no-default-features --features 'libsql postgres'",
            True,
            id="whitespace-separated-in-one-argument",
        ),
        pytest.param(
            "--no-default-features --features=libsql,postgres",
            True,
            id="joined-with-equals",
        ),
        pytest.param("--no-default-features -F postgres", True, id="short-flag"),
        pytest.param(
            "--no-default-features -F=postgres", True, id="short-flag-with-equals"
        ),
        pytest.param("--no-default-features -Fpostgres", True, id="short-flag-joined"),
        pytest.param(
            "--no-default-features -F libsql -F postgres", True, id="repeated-flags"
        ),
    ],
)
def test_every_spelling_of_a_feature_list_is_read(
    flags: str, *, expected: bool
) -> None:
    """Read Cargo's feature syntax, not one spelling of it.

    A leg whose features the reader cannot see resolves to the default set, so
    a narrow leg naming `postgres` explicitly would read as not bearing it and
    `test_every_postgres_bearing_leg_declares_it` would reject a correct
    declaration. The `narrow-and-named` case is the one that keeps this honest:
    it is the only narrow case expected to be false, so a reader that answered
    true for everything would fail it.
    """
    assert _enables_postgres(flags) is expected, (
        f"{flags!r} should read as postgres={expected}"
    )
