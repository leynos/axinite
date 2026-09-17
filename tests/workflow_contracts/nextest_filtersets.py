"""Reading what a nextest filterset names and what it selects.

Separated from ``nextest_config`` so the profile structure and the
filterset grammar stay legible apart, and so neither module outgrows
the 400-line limit ``AGENTS.md`` sets. Both readings are re-exported
from ``nextest_config``, because a reader of a budget wants one import
rather than two.

The two are different questions and the difference decides an
allowance. ``not binary(x)`` mentions ``x`` and selects the opposite of
it: the exclusion reading wants the mention, and the allowance reading
wants the selection.
"""

import re
import typing as typ

from nextest_errors import NextestConfigurationError

#: A ``binary(...)`` term inside a nextest filterset.
_BINARY_TERM: typ.Final[re.Pattern[str]] = re.compile(r"binary\(\s*([^)\s]+)\s*\)")


def binaries_named(filterset: object) -> frozenset[str]:
    """Return the test binaries a filterset names.

    Parsed as terms rather than searched as text, so a filterset naming
    ``binary(a) | binary(b)`` reports both and one naming neither
    reports nothing. Only the names matter here: whether a term is
    negated is the caller's question, because ``not binary(x)`` in a
    ``default-filter`` excludes the binary while the same term in an
    override's filter selects it.

    Parameters
    ----------
    filterset
        A ``filter`` or ``default-filter`` value, which need not be a
        string.

    Returns
    -------
    frozenset of str
        Every binary name the filterset mentions.

    Examples
    --------
    >>> sorted(binaries_named("binary(trybuild) | binary(ui)"))
    ['trybuild', 'ui']
    >>> binaries_named(None)
    frozenset()
    """
    match filterset:
        case str():
            return frozenset(_BINARY_TERM.findall(filterset))
        case _:
            return frozenset()


#: Every spelling that puts the term after it outside the selection.
#: cargo-nextest accepts three: the word ``not``, the prefix ``!``, and
#: the infix ``-``, which is set difference and excludes its right-hand
#: side. Reading only ``not`` left the other two selecting the binary
#: they exclude, which grants an override's allowance to a binary
#: nextest runs under the base timeout.
#:
#: ``not`` needs the word boundary and the following space, so a
#: predicate named ``nothing(...)`` is not read as a negation. ``!`` and
#: ``-`` are punctuation and take optional space instead.
_NEGATION: typ.Final[str] = r"(?:\bnot\s+|!\s*|-\s*)"

#: A ``binary(...)`` term under a negation, which excludes rather than
#: selects.
_NEGATED_BINARY_TERM: typ.Final[re.Pattern[str]] = re.compile(
    _NEGATION + r"binary\(\s*([^)\s]+)\s*\)"
)

#: A negation applied to a parenthesized group. The names inside it
#: cannot be attributed by a reader that matches terms, so a filterset
#: carrying one is refused rather than read.
_NEGATED_GROUP: typ.Final[re.Pattern[str]] = re.compile(_NEGATION + r"\(")


def binaries_selected(filterset: object) -> frozenset[str]:
    """Return the test binaries a filterset selects, negation honoured.

    Different from :func:`binaries_named`, and the difference decides
    whether a binary is allowed an override's budget.
    ``not binary(trybuild)`` mentions ``trybuild`` and selects the
    opposite of it, so a reader that only collected names would report
    an override as granting an allowance nextest never applies, and the
    binary would run under the base allowance while the contract
    certified it.

    Two shapes are refused rather than read, for the same reason. A
    negation over a parenthesized group, because attributing
    ``not (binary(a) | binary(b))`` needs the filterset evaluated rather
    than its terms matched. And a binary that is both selected and
    excluded, such as ``binary(a) | binary(b) - binary(b)``, because
    which occurrence wins depends on how the expression groups. A
    contract that cannot evaluate an expression must not certify the
    lane it guards.

    Parameters
    ----------
    filterset
        A ``filter`` or ``default-filter`` value, which need not be a
        string.

    Returns
    -------
    frozenset of str
        Every binary the filterset selects.

    Raises
    ------
    NextestConfigurationError
        If a negation covers a group, or a binary is both selected and
        excluded, so that this reading cannot attribute it.

    Examples
    --------
    >>> sorted(binaries_selected("binary(a) | binary(b)"))
    ['a', 'b']
    >>> sorted(binaries_selected("binary(a) & not binary(b)"))
    ['a']
    >>> binaries_selected(None)
    frozenset()
    """
    match filterset:
        case str():
            collapsed = " ".join(filterset.split())
        case _:
            return frozenset()
    excluded = frozenset(_NEGATED_BINARY_TERM.findall(collapsed))
    remaining = _NEGATED_BINARY_TERM.sub(" ", collapsed)
    if _NEGATED_GROUP.search(remaining):
        message = (
            f"the filterset {collapsed!r} negates a group, so which binaries "
            f"it selects cannot be read from its terms; this contract refuses "
            f"to report an allowance it cannot attribute"
        )
        raise NextestConfigurationError(message)
    selected = frozenset(_BINARY_TERM.findall(remaining))
    both = selected & excluded
    if both:
        message = (
            f"the filterset {collapsed!r} both selects and excludes "
            f"{sorted(both)}, and which wins depends on how the expression "
            f"groups, which this contract matches terms rather than "
            f"evaluates; it refuses to report an allowance it cannot "
            f"attribute"
        )
        raise NextestConfigurationError(message)
    return selected
