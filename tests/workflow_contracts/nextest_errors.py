"""What a nextest configuration can be wrong in.

Separated from ``nextest_config`` so that the duration grammar and the
configuration structure can both raise these without importing each
other, and so neither module outgrows the 400-line limit ``AGENTS.md``
sets. Both are re-exported from ``nextest_config``.
"""


class NextestConfigurationError(ValueError):
    """Raised when the configuration cannot be read as a set of budgets.

    Separate from a budget in the wrong order. A file that is not TOML,
    a profile declaring no ``slow-timeout``, or one whose
    ``global-timeout`` has been commented out, is a configuration this
    contract cannot reason about rather than one whose tiers are
    inverted.
    """


class UnboundedTestError(NextestConfigurationError):
    """Raised when a ``slow-timeout`` terminates no test.

    ``terminate-after`` is optional, and without it nextest marks a test
    slow and lets it run on, so the configuration parses, reads as
    deliberate, and bounds nothing. Reporting that as a period-long
    budget would put a number on the tier that is missing.
    """
