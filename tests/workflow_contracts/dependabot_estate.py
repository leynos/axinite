"""What this repository's Dependabot configuration must declare, by name.

Concordat's DB-005 rule audits `.github/dependabot.yml` at one revision;
nothing a later contributor edits has to pass it before it merges.
`dependabot_estate_test.py` applies the readings in `_dependabot_shape.py` to
the estate, and the values below are the estate it holds to.

Two kinds of value live here. The first are canon's: the ecosystems an entry
may declare, and the shape its groups must take. The second are this
repository's own policy, which canon declines to judge -- the labels each
entry asks for, the cooldown it applies, and the pull-request limit it keeps.
Pinning the second by value makes a change to any of them a change to this
module, which is what a policy change should look like.

The entry points are listed rather than derived from the file so that
deleting a stanza fails a named case instead of silently shortening a
parametrization, and the expected values are keyed by the same entry point so
a test can take one parameter and find everything it needs.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

from _workflow_policy import REPOSITORY_ROOT

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

CONFIG_PATH: typ.Final[Path] = REPOSITORY_ROOT / ".github" / "dependabot.yml"

#: The ecosystems this repository runs updates for. Canon leaves ecosystem
#: coverage to the governance rules, so deleting an entry whole would pass
#: every shape question, there being nothing left to ask, and this list is
#: what notices.
REQUIRED_ECOSYSTEMS: typ.Final[frozenset[str]] = frozenset(
    {"cargo", "pip", "bun", "github-actions"}
)

#: The one entry each estate contract is about, as an ecosystem and a
#: directory it must cover.
ENTRY_POINTS: typ.Final[tuple[tuple[str, str], ...]] = (
    ("cargo", "/"),
    ("pip", "/tests/e2e"),
    ("bun", "/web-src"),
    ("github-actions", "/"),
)

#: The labels and bounds each entry carries. Labels are how a dependency pull
#: request reaches the right reviewers, and the limit is how many such pull
#: requests one ecosystem may have open at once.
ENTRY_POLICY: typ.Final[dict[tuple[str, str], tuple[frozenset[str], int]]] = {
    ("cargo", "/"): (frozenset({"dependencies", "cargo"}), 5),
    ("pip", "/tests/e2e"): (frozenset({"dependencies", "pip"}), 5),
    ("bun", "/web-src"): (frozenset({"dependencies", "bun"}), 5),
    ("github-actions", "/"): (
        frozenset({"dependencies", "github-actions"}),
        5,
    ),
}

#: The cooldown each entry applies, in days, keyed by the cooldown key. A
#: cooldown keeps a release from arriving the hour it is published, which is
#: when a compromised or yanked release is most likely to be taken. The
#: `github-actions` entry declares no major cooldown of its own; canon gives no
#: reason to, and this table records that rather than inventing one.
ENTRY_COOLDOWN: typ.Final[dict[tuple[str, str], dict[str, int]]] = {
    ("cargo", "/"): {"default-days": 7, "semver-major-days": 14},
    ("pip", "/tests/e2e"): {"default-days": 7, "semver-major-days": 14},
    ("bun", "/web-src"): {"default-days": 7, "semver-major-days": 14},
    ("github-actions", "/"): {"default-days": 7},
}
