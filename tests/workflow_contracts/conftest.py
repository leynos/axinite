"""Fixtures shared by more than one contract in this directory.

The nextest profile reading lived in three modules as three identical
copies. Copies of a fixture drift: one module's reading can be pointed
at a different configuration, or given a different scope, while the
others keep the old behaviour and nothing says the suite is now
reading two different things. A single definition here is what the
repository's test conventions ask for, and it is also what makes the
`module` scope honest, since the parse then happens once per module
that asks for it rather than once per copy.

`fixtures/` in this directory holds data rather than pytest fixtures,
so this is the right home.
"""

import pytest
from nextest_config import Profile, profiles_of
from timeout_budgets import NEXTEST_CONFIG


@pytest.fixture(scope="module")
def nextest_profiles() -> dict[str, Profile]:
    """Return each nextest profile the configuration declares.

    Read through the acquisition helper rather than with `read_text`,
    so a configuration that cannot be read is reported as this suite
    reports every other unreadable source, and so the reading is
    written once rather than in each module that needs it.

    Returns
    -------
    dict[str, Profile]
        Profile name to its table and overrides.
    """
    return profiles_of(NEXTEST_CONFIG)
