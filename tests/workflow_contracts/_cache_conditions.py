"""What a cache save step's `if` expression has to say, read in full.

One key, one writer, one archive. The expression that arranges it is four
conjuncts and nothing else: the writing matrix leg, the push event, the `main`
ref, and the step's own restore result. Reading it by searching for substrings
is what this module exists to stop. `github.event_name == 'push' ||
github.event_name == 'schedule'` contains the push text; `matrix.name ==
'default' && matrix.name == 'all-features'` contains the leg text; and
`steps.cargo-registry.outputs.cache-hit == 'true'` contains the cache-hit
reference while meaning its exact opposite, which would skip every write the
archive actually needed. Each of those passes a substring reading and none of
them is the approved policy.

So the condition is taken apart instead. It is split on `&&`, and the set of
conjuncts is compared with the approved set: anything missing fails, anything
extra fails, and any `||` or grouping fails outright, because a disjunction
cannot be judged conjunct by conjunct and is not part of the policy.

`cache_condition_test.py` drives this module directly, with a mutation of each
conjunct, because the estate's own two save steps are correct and a reader
parametrized over correct input discriminates nothing.
"""

from __future__ import annotations

import re

#: The matrix leg allowed to publish the registry archive. It resolves the
#: widest dependency graph, so its archive is a superset of the others'; two
#: legs saving would race for one key and the later upload would win by
#: accident.
WRITING_LEG = "matrix.name == 'all-features'"

#: The event that owns the key. `github.ref` reads `refs/heads/main` for a
#: manual dispatch against main just as it does for a push, so a guard on the
#: ref alone would let a warm-cache measurement overwrite what the merge
#: wrote.
PUSH_EVENT = "github.event_name == 'push'"

#: The branch that owns the key. Pull requests restore and never save.
MAIN_REF = "github.ref == 'refs/heads/main'"

#: The approved conjuncts that name no step. The cache-hit conjunct is
#: matched separately because it carries the restore step's ID.
FIXED_CONJUNCTS: frozenset[str] = frozenset({WRITING_LEG, PUSH_EVENT, MAIN_REF})

#: The save's reference to its own restore step's outcome, in any form. Used
#: to recognize the conjunct; `CACHE_MISS_RE` decides whether it is right.
CACHE_HIT_RE: re.Pattern[str] = re.compile(
    r"steps\.(?P<id>[A-Za-z0-9_-]+)\.outputs\.cache-hit"
)

#: The only acceptable form of that conjunct: the restore missed. The whole
#: match is anchored, so `== 'true'` is a fault rather than a near miss, and
#: so is an extra term smuggled into the same conjunct.
CACHE_MISS_RE: re.Pattern[str] = re.compile(
    r"^steps\.(?P<id>[A-Za-z0-9_-]+)\.outputs\.cache-hit != 'true'$"
)

#: What separates one conjunct from the next, and the operator that makes the
#: expression unreadable this way.
CONJUNCTION = "&&"
DISJUNCTION = "||"


def conjuncts(condition: str) -> tuple[str, ...]:
    """Return the `&&`-separated terms of an expression, whitespace collapsed.

    Empty terms are kept rather than dropped. `a && && b` and `a &&` are not
    expressions GitHub evaluates, and a reader that silently discarded the
    empty piece would hand `save_condition_faults` a conjunct set identical to
    the well-formed condition's, so a malformed guard would pass the
    exact-predicate comparison.

    Parameters
    ----------
    condition
        A step's `if` expression. The estate writes these as folded YAML
        scalars, so the text arrives with its line breaks already joined.

    Returns
    -------
    tuple of str
        Each term, in order, including any empty one a stray operator
        produces. An expression with no operator at all yields one term, so
        the empty condition yields one empty term.
    """
    joined = " ".join(condition.split())
    return tuple(term.strip() for term in joined.split(CONJUNCTION))


def _cache_hit_faults(terms: tuple[str, ...], restore_ids: frozenset[str]) -> list[str]:
    """Return what is wrong with the condition's reference to its own restore.

    Parameters
    ----------
    terms
        The condition's conjuncts.
    restore_ids
        The IDs the job's own registry restore steps declare.

    Returns
    -------
    list of str
        One sentence per fault, empty when the conjunct is the approved one.
    """
    referring = [term for term in terms if CACHE_HIT_RE.search(term)]
    if not referring:
        return [
            "it does not consult its restore step's `cache-hit` output, so it "
            "re-uploads an archive it already has on every push"
        ]
    if len(referring) > 1:
        return [
            f"it consults `cache-hit` {len(referring)} times ({referring}); the "
            "approved predicate names the restore step once"
        ]
    term = referring[0]
    exact = CACHE_MISS_RE.match(term)
    if exact is None:
        return [
            f"its cache-hit term is {term!r}, not "
            "`steps.<restore-id>.outputs.cache-hit != 'true'`; the inverted "
            "comparison skips the write the key needs, and a term with "
            "anything else joined to it is not this predicate"
        ]
    named = exact["id"]
    if named not in restore_ids:
        # A step ID nothing declares is not an error in Actions: the
        # expression resolves to the empty string, the inequality holds, and
        # the archive is re-uploaded on every push. Nothing fails, so the cost
        # is the only evidence.
        return [
            f"it reads `steps.{named}.outputs.cache-hit`, but no restore step "
            f"in that job declares that ID; it declares {sorted(restore_ids)}. "
            "The expression resolves to the empty string and the guard is dead"
        ]
    return []


def _shape_faults(collapsed: str, terms: tuple[str, ...]) -> list[str]:
    """Return what is wrong with an expression before its terms are compared.

    These are the readings that make the conjunct comparison meaningless
    rather than merely failing it: there is nothing to compare, or the
    expression is not a plain conjunction at all.

    Parameters
    ----------
    collapsed
        The `if` expression with its whitespace collapsed.
    terms
        Its `&&`-separated terms, empty pieces included.

    Returns
    -------
    list of str
        One sentence, or none when the expression is a well-formed
        conjunction whose terms can be compared with the approved set.
    """
    if not collapsed:
        return ["it carries no condition at all, so every leg writes the key"]
    if DISJUNCTION in collapsed:
        return [
            f"it contains {DISJUNCTION!r}. An alternative arm can admit another "
            "event or another leg while every approved term is still present, "
            "so a disjunction is refused rather than judged term by term"
        ]
    if "(" in collapsed or ")" in collapsed:
        return [
            "it is grouped with parentheses, which this reader does not take "
            "apart; the approved predicate is four plain conjuncts"
        ]
    if not all(terms):
        return [
            f"it has a stray {CONJUNCTION!r}: {collapsed!r} splits into "
            f"{list(terms)}, one of which is empty. GitHub does not evaluate "
            "that expression, and a reader that dropped the empty piece would "
            "compare the same conjunct set as the well-formed condition"
        ]
    return []


def save_condition_faults(condition: str, restore_ids: frozenset[str]) -> list[str]:
    """Return every way a save condition departs from the approved predicate.

    Parameters
    ----------
    condition
        The save step's `if` expression.
    restore_ids
        The IDs the job's own registry restore steps declare.

    Returns
    -------
    list of str
        One sentence per fault, empty when the condition is exactly the
        approved push-to-main, all-features, cache-miss predicate.
    """
    collapsed = " ".join(condition.split())
    terms = conjuncts(collapsed)
    shape = _shape_faults(collapsed, terms)
    if shape:
        return shape
    faults = _cache_hit_faults(terms, restore_ids)
    fixed = frozenset(term for term in terms if not CACHE_HIT_RE.search(term))
    faults.extend(
        f"it does not require {missing}" for missing in sorted(FIXED_CONJUNCTS - fixed)
    )
    faults.extend(
        f"it carries the extra term {extra!r}; the approved predicate is "
        "exactly the writing leg, the push event, the main ref and the "
        "restore step's cache miss"
        for extra in sorted(fixed - FIXED_CONJUNCTS)
    )
    return faults
