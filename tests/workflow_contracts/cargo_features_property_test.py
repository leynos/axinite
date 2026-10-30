"""Property contracts for Cargo feature selection in coverage legs."""

from __future__ import annotations

import shlex
from dataclasses import dataclass

from _cargo_features import _enables_postgres
from hypothesis import given, settings
from hypothesis import strategies as st


@dataclass(frozen=True)
class FeatureCase:
    """A Cargo feature selection and one rendering of its arguments."""

    features: list[str]
    separator: str
    form: str
    no_defaults: bool
    default_postgres: bool
    repeated: bool
    all_features: bool


@settings(max_examples=100)
@given(
    case=st.builds(
        FeatureCase,
        features=st.lists(
            st.sampled_from(["postgres", "libsql", "embedded-postgres"]),
            min_size=1,
            max_size=6,
        ),
        separator=st.sampled_from([",", " "]),
        form=st.sampled_from(
            ["long", "long-equals", "short", "short-equals", "joined"]
        ),
        no_defaults=st.booleans(),
        default_postgres=st.booleans(),
        repeated=st.booleans(),
        all_features=st.booleans(),
    )
)
def test_equivalent_feature_flag_forms_resolve_postgres(case: FeatureCase) -> None:
    """Read the same feature set across Cargo's supported flag spellings."""
    value = case.separator.join(case.features)
    quoted = shlex.quote(value)
    forms = {
        "long": f"--features {quoted}",
        "long-equals": f"--features={quoted}",
        "short": f"-F {quoted}",
        "short-equals": f"-F={quoted}",
        "joined": f"-F{quoted}",
    }
    flags = [forms[case.form]]
    if case.repeated:
        flags.append(f"-F {shlex.quote(case.features[0])}")
    if case.no_defaults:
        flags.append("--no-default-features")
    if case.all_features:
        flags.append("--all-features")
    defaults = frozenset({"postgres"} if case.default_postgres else set())
    expected = (
        case.all_features
        or "postgres" in case.features
        or (case.default_postgres and not case.no_defaults)
    )
    rendered = " ".join(flags)
    assert _enables_postgres(rendered, defaults) is expected, (
        f"{rendered!r} with defaults {sorted(defaults)!r} should read as "
        f"postgres={expected}"
    )
