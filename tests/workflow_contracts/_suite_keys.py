"""What a single command selects: its features and its nextest profile.

Two runs are the same work when they cover the same scope, under the same
feature selection, under the same profile. This module answers the second and
third of those for one command, and `_suite_reader.py` uses it to key every
run the estate dispatches.

Feature sets are compared as sets, so a leg rewritten from `--features a,b` to
`--features a --features b` is still the same run, and `--all-features` stays
distinct from a list that happens to name every feature today, because
tomorrow it will not. A command that does not pass `--no-default-features` is
keyed with the root manifest's `default` list folded in, because Cargo enables
those whether the command names them or not: without that, a leg naming three
members of `default` read as different work from the leg naming none, and
`test.yml` ran both.
"""

from __future__ import annotations

import shlex

from _suite_targets import (
    DEFAULT_FEATURES,
    DEFAULT_PROFILE,
    FEATURE_VARIABLE,
    PROFILE_VARIABLE,
)

#: Sentinels for the flags that select features without naming any. They are
#: part of the key so that `--all-features` and `--no-default-features
#: --features libsql` cannot collide with each other or with a feature list.
ALL_FEATURES = ":all-features"
NO_DEFAULT_FEATURES = ":no-default-features"


def features_named_by(token: str, following: tuple[str, ...]) -> tuple[str, ...]:
    """Return the features one argument selects.

    Parameters
    ----------
    token
        One shell word of the command.
    following
        The word after it, when there is one. `--features` takes its value
        separately; the other spellings carry it.

    Returns
    -------
    tuple of str
        The feature names, or a sentinel for the flags that select features
        without naming any. Empty for every other argument: a profile, an
        output path or a test filter changes how a run is reported, not which
        tests it compiles and executes.
    """
    if token == "--all-features":
        return (ALL_FEATURES,)
    if token == "--no-default-features":
        return (NO_DEFAULT_FEATURES,)
    if token == "--features" and following:
        return tuple(following[0].split(","))
    if token.startswith("--features="):
        return tuple(token.partition("=")[2].split(","))
    return ()


def unpack_feature_variable(tokens: list[str]) -> list[str]:
    """Return the tokens with `TEST_FEATURES="..."` expanded in place.

    A `make` step passes the selection as one assignment word. Unpacking it
    here, rather than at the call site, means the two command shapes are keyed
    the same way and a coverage lane can be compared with a test lane.

    Parameters
    ----------
    tokens
        A command's arguments, already split into shell words.

    Returns
    -------
    list of str
        The same words, with any `TEST_FEATURES=` assignment replaced by the
        words it carries. Every other token is passed through unchanged.
    """
    return [
        part
        for token in tokens
        for part in (
            shlex.split(token.partition("=")[2])
            if token.startswith(f"{FEATURE_VARIABLE}=")
            else [token]
        )
    ]


def feature_key(args: str) -> frozenset[str]:
    """Return the feature selection a command's arguments make.

    Parameters
    ----------
    args
        The command's arguments, with every matrix reference already
        substituted.

    Returns
    -------
    frozenset of str
        The features the command actually enables: each named feature, the
        root manifest's defaults unless the command turns them off, and a
        sentinel for `--all-features` and for `--no-default-features`.
    """
    tokens = unpack_feature_variable(shlex.split(args, comments=False, posix=True))
    selected = {
        name
        for index, token in enumerate(tokens)
        for name in features_named_by(token, tuple(tokens[index + 1 : index + 2]))
    }
    named = frozenset(name for name in selected if name)
    # `--no-default-features` says the defaults are off, so the explicit list
    # is the whole of the selection. Everything else gets them whether it
    # names them or not, which is the point: a leg naming three members of
    # `default` is the leg that names nothing. `--all-features` needs no
    # exception, because its sentinel already keeps it apart from every list.
    if NO_DEFAULT_FEATURES in named:
        return named
    return named | DEFAULT_FEATURES


def profile_of(args: str) -> str:
    """Return the nextest profile a command's arguments select.

    Parameters
    ----------
    args
        The command's arguments, with every matrix reference already
        substituted. Both spellings are read: `--profile ci` on a Cargo
        command, and `NEXTEST_PROFILE=ci` on a Make target.

    Returns
    -------
    str
        The profile name, or the nextest default when the command names none.
    """
    tokens = shlex.split(args, comments=False, posix=True)
    for index, token in enumerate(tokens):
        if token == "--profile" and index + 1 < len(tokens):
            return tokens[index + 1]
        if token.startswith("--profile="):
            return token.partition("=")[2]
        if token.startswith(f"{PROFILE_VARIABLE}="):
            return token.partition("=")[2]
    return DEFAULT_PROFILE
