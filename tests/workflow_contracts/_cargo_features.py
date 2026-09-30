"""Read which Cargo features a coverage leg's flags compile.

The coverage contracts need to know whether a leg bears the `postgres`
feature, which is a default feature and so is enabled by any leg that does not
pass `--no-default-features`. This module reads Cargo's feature syntax from a
leg's flags and the root package's default set from its manifest.
"""

from __future__ import annotations

import re
import shlex
import typing as typ

import tomllib
from _workflow_policy import REPOSITORY_ROOT

#: The feature whose presence makes a leg Postgres-bearing.
POSTGRES_FEATURE: typ.Final[str] = "postgres"


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
