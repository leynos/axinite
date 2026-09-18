"""Resolving which overrides a nextest profile inherits.

Separated from ``nextest_config`` so the parsing and the inheritance
walk stay legible apart, and so neither module outgrows the 400-line
limit ``AGENTS.md`` sets.

Every profile inherits ``default`` unless ``inherits`` names another
parent, and that parent may name a third. The chain is walked rather
than assumed to be one step: a reading that copied ``default``'s
overrides alone would miss the middle of a chain and could approve a
whole-run budget below the per-test allowance actually in force.
"""

import typing as typ

from nextest_errors import NextestConfigurationError

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from nextest_config import Profile


def _ancestors(name: str, declared: "dict[str, Profile]") -> tuple[str, ...]:
    """Return one profile's ancestors, nearest first, ending at default.

    Every profile inherits ``default`` unless ``inherits`` names another
    parent, and that parent may name a third, so the chain is walked
    rather than assumed to be one step. ``default`` has no parent.

    Parameters
    ----------
    name
        The profile to walk from.
    declared
        Each profile as its own table declares it.

    Returns
    -------
    tuple of str
        The ancestors, nearest first.

    Raises
    ------
    NextestConfigurationError
        If ``inherits`` is not a string, names a profile the file does
        not declare, or closes a cycle.
    """
    chain: list[str] = []
    seen = {name}
    current = name
    while current != "default":
        declared_parent = declared[current].own.get("inherits")
        match declared_parent:
            case None:
                parent = "default"
            case str():
                parent = declared_parent
            case _:
                message = (
                    f"profile.{current}.inherits is {declared_parent!r}, not a "
                    f"profile name; nextest refuses the file, so no budget can "
                    f"be read from it"
                )
                raise NextestConfigurationError(message)
        if parent not in declared:
            message = (
                f"profile.{current} inherits {parent!r}, which the "
                f"configuration does not declare; nextest refuses the file"
            )
            raise NextestConfigurationError(message)
        if parent in seen:
            walked = " -> ".join([name, *chain, parent])
            message = (
                f"profile.{current} inherits {parent!r}, closing the cycle "
                f"{walked}; nextest refuses the file"
            )
            raise NextestConfigurationError(message)
        chain.append(parent)
        seen.add(parent)
        current = parent
    return tuple(chain)


def with_inherited_overrides(declared: "dict[str, Profile]") -> "dict[str, Profile]":
    """Return each profile carrying the overrides it inherits.

    nextest consults an ancestor's ``[[overrides]]`` for whichever
    profile is selected, after that profile's own, so a test can be
    governed by an override the selected profile never declares. The
    whole chain is walked: a profile naming ``inherits = "ci"`` takes
    ``ci``'s overrides and then ``default``'s, and a reading that copied
    ``default``'s alone would miss the middle of the chain and could
    approve a whole-run budget below the per-test allowance in force.

    ``default`` inherits nothing, because its own overrides are already
    recorded once.

    Parameters
    ----------
    declared
        Each profile as its own table declares it.

    Returns
    -------
    dict of str to Profile
        The same profiles, with ``inherited`` filled in.

    Raises
    ------
    NextestConfigurationError
        If any profile's ``inherits`` cannot be resolved.
    """
    if "default" not in declared:
        return declared
    resolved: dict[str, Profile] = {}
    for name, profile in declared.items():
        inherited = tuple(
            (f"profile.{ancestor}", table)
            for ancestor in _ancestors(name, declared)
            for table in declared[ancestor].overrides
        )
        resolved[name] = profile._replace(inherited=inherited)
    return resolved
