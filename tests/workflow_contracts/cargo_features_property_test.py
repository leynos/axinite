"""Property contracts for Cargo feature selection in coverage legs."""

from __future__ import annotations

import shlex

from _cargo_features import _enables_postgres
from hypothesis import given, settings
from hypothesis import strategies as st


@settings(max_examples=100)
@given(
    features=st.lists(
        st.sampled_from(["postgres", "libsql", "embedded-postgres"]),
        min_size=1,
        max_size=6,
    ),
    separator=st.sampled_from([",", " "]),
    form=st.sampled_from(["long", "long-equals", "short", "short-equals", "joined"]),
    no_defaults=st.booleans(),
    default_postgres=st.booleans(),
    repeated=st.booleans(),
    all_features=st.booleans(),
)
def test_equivalent_feature_flag_forms_resolve_postgres(
    features: list[str],
    separator: str,
    form: str,
    *,
    no_defaults: bool,
    default_postgres: bool,
    repeated: bool,
    all_features: bool,
) -> None:
    """Read the same feature set across Cargo's supported flag spellings."""
    value = separator.join(features)
    quoted = shlex.quote(value)
    forms = {
        "long": f"--features {quoted}",
        "long-equals": f"--features={quoted}",
        "short": f"-F {quoted}",
        "short-equals": f"-F={quoted}",
        "joined": f"-F{quoted}",
    }
    flags = [forms[form]]
    if repeated:
        flags.append(f"-F {shlex.quote(features[0])}")
    if no_defaults:
        flags.append("--no-default-features")
    if all_features:
        flags.append("--all-features")
    defaults = frozenset({"postgres"} if default_postgres else set())
    expected = all_features or "postgres" in features or (
        default_postgres and not no_defaults
    )
    assert _enables_postgres(" ".join(flags), defaults) is expected
