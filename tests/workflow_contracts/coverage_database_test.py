"""Contracts binding the coverage job's database URL to the name tests read.

`src/testing/postgres.rs` reads `TEST_DATABASE_URL`, and falls back to
`postgresql://localhost/axinite_test` when it is absent. That fallback carries
no user and no password, so the pool fails with
`kind: Config, cause: "password missing"`.

The failure is loud by design rather than by accident: `is_database_unavailable`
lists only transport and name-resolution failures, deliberately excluding
authentication and configuration errors, so a misconfigured job fails instead
of quietly reporting coverage for tests that never ran.

That is what makes the export worth a contract. It was renamed to
`DATABASE_URL` in #243 on 2026-07-14, which nothing on the test path reads, and
every push to `main` failed from two days later until this was fixed. Nothing
caught it, because the workflow still exported something plausible and the
`libsql-only` leg, which needs no database, stayed green. See issue #350.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import shlex
import tomllib
import typing as typ
from urllib.parse import urlsplit

import pytest

from _workflow_files import load
from _workflow_policy import REPOSITORY_ROOT, WORKFLOW_DIR, step_text

WORKFLOW: typ.Final[str] = "coverage.yml"

#: The variable `src/testing/postgres.rs` reads. Renaming the export without
#: renaming the reader is the exact mistake this file exists to catch, so the
#: constant is spelled out here rather than derived from the workflow.
TEST_URL_VARIABLE: typ.Final[str] = "TEST_DATABASE_URL"

#: The job that runs the Postgres-bearing coverage legs.
JOB: typ.Final[str] = "coverage"

#: Matches the shell assignment of a URL to a variable, so the value tied to
#: `TEST_DATABASE_URL` can be checked rather than any credentialed URL that
#: happens to appear in the same script. A passwordless `TEST_DATABASE_URL`
#: beside a credentialed `DATABASE_URL` reproduces the original failure exactly,
#: and a contract that searched the joined script would pass it.
ASSIGNMENT_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"^\s*(?P<name>[A-Za-z_][A-Za-z0-9_]*)=\"?(?P<value>[^\"\n]+?)\"?\s*$",
    re.MULTILINE,
)

#: Matches `echo "NAME=${shell_var}" >> "$GITHUB_ENV"`, which is how a value
#: reaches later steps. The exported name and the shell variable holding the
#: value are both captured, so the two halves can be joined.
#:
#: The redirection target is part of the pattern on purpose. A line redirecting
#: to an ordinary file looks identical up to the `>>`, and would satisfy every
#: assertion here while later steps received nothing and the helper fell back to
#: the unauthenticated URL.
EXPORT_RE: typ.Final[re.Pattern[str]] = re.compile(
    r"echo\s+\"(?P<name>[A-Za-z_][A-Za-z0-9_]*)=\$\{(?P<source>[A-Za-z_]"
    r"[A-Za-z0-9_]*)\}\"\s*>>\s*\"?\$(?:\{)?GITHUB_ENV(?:\})?\"?",
)


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


def _service_env() -> dict[str, object]:
    """Return the Postgres service container's environment."""
    services = _postgres_job().get("services")
    assert isinstance(services, dict), f"{JOB} must declare a services mapping"
    postgres = services.get("postgres")
    assert isinstance(postgres, dict), f"{JOB} must declare a postgres service"
    env = postgres.get("env")
    assert isinstance(env, dict), "the postgres service must declare env"
    return env


def _exporting_step() -> dict[str, object]:
    """Return the single step that exports the test database URL."""
    matches = [
        step
        for step in _steps(_postgres_job())
        if f"{TEST_URL_VARIABLE}=" in step_text(step)
    ]
    assert len(matches) == 1, (
        f"expected exactly one step exporting {TEST_URL_VARIABLE}, found {len(matches)}"
    )
    return matches[0]


def _exported_test_url() -> str:
    """Return the URL value the step exports as `TEST_DATABASE_URL`."""
    # The script assigns the URL to a shell variable and then exports that
    # variable, so both halves are resolved rather than assumed. Following the
    # indirection is the point: it is what ties the credentials being asserted
    # to the name the tests read.
    script = step_text(_exporting_step())
    exports = {m["name"]: m["source"] for m in EXPORT_RE.finditer(script)}
    source = exports.get(TEST_URL_VARIABLE)
    assert source is not None, (
        f"the step must export {TEST_URL_VARIABLE} from a shell variable, as "
        f'echo "{TEST_URL_VARIABLE}=${{...}}" >> "$GITHUB_ENV"'
    )
    assignments = {
        m["name"]: m["value"]
        for m in ASSIGNMENT_RE.finditer(script)
        if not m.group(0).lstrip().startswith("echo")
    }
    value = assignments.get(source)
    assert value is not None, (
        f"{TEST_URL_VARIABLE} is exported from ${source}, which the step never assigns"
    )
    return value


def test_the_coverage_job_exports_the_variable_the_tests_read() -> None:
    """Export the name the code reads, not one that merely looks right.

    A plausible but unread name is worse than no export at all: the job still
    runs, the tests still fail, and the workflow reports a database problem
    rather than a configuration one.
    """
    script = step_text(_exporting_step())
    assert f"{TEST_URL_VARIABLE}=" in script, (
        f"{JOB} must export {TEST_URL_VARIABLE}, which "
        "src/testing/postgres.rs reads. Without it the helper falls back to a "
        "URL with no credentials and every Postgres test fails on "
        "'password missing'."
    )


def test_the_exported_test_url_matches_the_service_container() -> None:
    """Check the URL bound to `TEST_DATABASE_URL`, not any URL nearby.

    Searching the whole script would pass a passwordless `TEST_DATABASE_URL`
    sitting beside a credentialed `DATABASE_URL`, which reproduces the original
    failure exactly while every other assertion here holds. The expected values
    come from the service container rather than being written out again, so the
    two cannot drift apart.
    """
    env = _service_env()
    parsed = urlsplit(_exported_test_url())
    assert parsed.username == env.get("POSTGRES_USER"), (
        f"the {TEST_URL_VARIABLE} value must carry the service's "
        f"POSTGRES_USER, got {parsed.username!r}"
    )
    assert parsed.password == env.get("POSTGRES_PASSWORD"), (
        f"the {TEST_URL_VARIABLE} value must carry the service's "
        "POSTGRES_PASSWORD. Without a password the pool fails with "
        '`kind: Config, cause: "password missing"`, which is the failure '
        "this contract exists to prevent."
    )
    assert parsed.path.lstrip("/") == env.get("POSTGRES_DB"), (
        f"the {TEST_URL_VARIABLE} value must name the service's POSTGRES_DB, "
        f"got {parsed.path!r}"
    )
    assert parsed.hostname == "localhost", (
        f"the service container is published on localhost; got {parsed.hostname!r}"
    )


def test_the_export_is_guarded_to_the_postgres_legs() -> None:
    """Only the legs with a database configured may advertise one.

    `services` is declared at job level, so the container starts for every
    matrix leg including `libsql-only`. That leg is built with
    `--no-default-features --features libsql` and must not be pointed at a
    database it does not use, which is what `has_postgres` expresses.
    """
    assert _exporting_step().get("if") == "matrix.has_postgres", (
        f"the {TEST_URL_VARIABLE} export must be guarded on "
        "matrix.has_postgres, so the libsql-only leg is not handed a database "
        "URL its feature set does not use"
    )


#: The variable a lane sets to say that Postgres is not optional on this run.
#: `src/testing/postgres.rs` skips its Postgres tests when the database is
#: unreachable, which is what a developer without one needs. On a lane that
#: starts a Postgres service and points the tests at it, the same skip reports
#: success for tests that never connected.
REQUIRE_VARIABLE: typ.Final[str] = "AXINITE_REQUIRE_POSTGRES"

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


def test_the_leg_that_provides_postgres_tells_the_tests_it_is_not_optional() -> None:
    """The database URL and the promise of a database ship in one step.

    Separating them is the failure this guards. A lane that exports the URL
    without the promise leaves the skip available, so a Postgres service that
    failed to start reports success for every test that needed it; a lane that
    exports the promise without the URL fails on the passwordless fallback,
    which is issue #350 again.
    """
    step = _exporting_step()
    script = step_text(step)
    assert f"{REQUIRE_VARIABLE}=" in script, (
        f"the step exporting {TEST_URL_VARIABLE} must also export "
        f"{REQUIRE_VARIABLE}, so a leg cannot receive the database without "
        "also being told that the database is not optional"
    )
    assert REQUIRE_APPEND_RE.search(script), (
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
