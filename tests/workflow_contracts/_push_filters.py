"""Whether a workflow's push trigger admits a branch, read as GitHub reads it.

A cache writer is worth nothing if a push to `main` never runs its workflow,
and the branch filter is where that is decided. Checking only for `main` in a
`branches` list reads `branches-ignore: [main]`, a `branches: ['**', '!main']`
list, and a push filtered on tags alone as triggers that reach `main`, and
all three refuse it.

So the filter is evaluated the way GitHub documents it. `branches` patterns are
read in order and the last one matching decides, a leading `!` excluding;
`branches-ignore` excludes any branch one of its patterns matches; the two
together are an error GitHub refuses; and a push that filters only tags does
not run for branches at all. A pattern is a glob in GitHub's grammar: `**`
crosses `/`, `*` does not, `?` and `+` quantify the preceding character, and
`[...]` is a class. A pattern this cannot compile is refused rather than read
as matching or not, since either guess could pass a writer that never runs.

`cache_policy_test.py` drives this directly; the estate's own writers are
all triggered on `main` and cannot discriminate.
"""

from __future__ import annotations

import re
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping

#: The glob tokens GitHub gives a meaning, longest first so `**` is not read
#: as two `*`. Everything between them is literal.
_GLOB_TOKEN_RE: typ.Final[re.Pattern[str]] = re.compile(r"\*\*|\*|\?|\+|\[[^\]]*\]")

#: What each quantifier and wildcard becomes in a regular expression. A class
#: is carried over as written.
_GLOB_TRANSLATION: typ.Final[dict[str, str]] = {
    "**": ".*",
    "*": "[^/]*",
    "?": "?",
    "+": "+",
}

#: The keys that filter a push by tag. A push naming one of these and no
#: branch filter runs for tags only.
_TAG_FILTERS: typ.Final[frozenset[str]] = frozenset({"tags", "tags-ignore"})


class PushFilterError(ValueError):
    """A push trigger in a shape this reading refuses to guess about."""


def push_reaches_branch(declared: Mapping[object, object], branch: str) -> bool:
    """Report whether a parsed `on:` mapping runs on a push to a branch.

    Parameters
    ----------
    declared
        A workflow's parsed `on:` mapping.
    branch
        The branch name, such as ``main``.

    Returns
    -------
    bool
        True when a push to that branch runs the workflow.

    Raises
    ------
    PushFilterError
        If the push trigger is not a mapping, declares both `branches` and
        `branches-ignore`, or holds a pattern that does not compile.

    Examples
    --------
    >>> push_reaches_branch({"push": {"branches-ignore": ["main"]}}, "main")
    False
    """
    # Membership, not the value. PyYAML reads a valueless `push:` as `None`,
    # which is exactly what a missing key returns, and the two mean the
    # broadest possible trigger and none at all.
    if "push" not in declared:
        return False
    push = declared["push"]
    if push is None:
        return True
    if not isinstance(push, dict):
        message = f"a push trigger of unmodelled shape {push!r}"
        raise PushFilterError(message)
    return _branch_filter_admits(push, branch)


def _branch_filter_admits(push: Mapping[object, object], branch: str) -> bool:
    """Apply whichever branch filter a push mapping declares."""
    if "branches" in push and "branches-ignore" in push:
        message = "declares both `branches` and `branches-ignore`"
        raise PushFilterError(message)
    if "branches" in push:
        return _last_match_admits(_patterns(push["branches"]), branch)
    if "branches-ignore" in push:
        ignored = _patterns(push["branches-ignore"])
        return not any(glob_matches(pattern, branch) for pattern in ignored)
    return not (_TAG_FILTERS & push.keys())


def _patterns(declared: object) -> tuple[str, ...]:
    """Return a filter's patterns, a single string being one pattern."""
    if isinstance(declared, str):
        return (declared,)
    if isinstance(declared, list) and all(isinstance(item, str) for item in declared):
        return tuple(declared)
    message = f"a branch filter of unmodelled shape {declared!r}"
    raise PushFilterError(message)


def _last_match_admits(patterns: tuple[str, ...], branch: str) -> bool:
    """Read a `branches` list in order; the last pattern matching decides."""
    admitted = False
    for pattern in patterns:
        negated = pattern.startswith("!")
        if glob_matches(pattern.removeprefix("!"), branch):
            admitted = not negated
    return admitted


def glob_matches(pattern: str, ref: str) -> bool:
    """Report whether a GitHub filter glob matches a ref name in full.

    Parameters
    ----------
    pattern
        One filter pattern, without a leading `!`.
    ref
        A branch or tag name.

    Returns
    -------
    bool
        True when the pattern matches the whole name.

    Raises
    ------
    PushFilterError
        If the pattern does not compile, such as one opening with `+`.

    Examples
    --------
    >>> glob_matches("release/**", "release/v1/rc")
    True
    >>> glob_matches("release/*", "release/v1/rc")
    False
    """
    try:
        return re.fullmatch(_glob_regex(pattern), ref) is not None
    except re.error as error:
        message = f"the filter pattern {pattern!r} does not compile ({error})"
        raise PushFilterError(message) from error


def _glob_regex(pattern: str) -> str:
    """Translate a GitHub filter glob into a regular expression."""
    parts: list[str] = []
    position = 0
    for token in _GLOB_TOKEN_RE.finditer(pattern):
        parts.append(re.escape(pattern[position : token.start()]))
        parts.append(_GLOB_TRANSLATION.get(token[0], token[0]))
        position = token.end()
    parts.append(re.escape(pattern[position:]))
    return "".join(parts)
