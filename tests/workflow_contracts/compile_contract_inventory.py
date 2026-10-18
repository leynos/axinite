"""Separate required contract targets from tests that drive nested Cargo."""

import typing as typ
from pathlib import Path

from contract_sources import matching_entries, read_source

#: Targets that must remain in workflow, group and profile contracts.
REQUIRED_COMPILE_CONTRACT_BINARIES: typ.Final[frozenset[str]] = frozenset(
    {"trybuild", "schema_helpers_ui"}
)

#: Required targets whose tests invoke nested Cargo to compile fixtures.
REQUIRED_NESTED_COMPILER_BINARIES: typ.Final[frozenset[str]] = frozenset(
    {"trybuild"}
)

#: The source token that identifies a trybuild-driven test target.
_NESTED_COMPILER_MARKER: typ.Final[str] = "trybuild::TestCases"


def compile_contract_binaries(tests_directory: Path) -> frozenset[str]:
    """Discover targets that invoke nested Cargo rather than every contract.

    This inventory is source-derived because the longer timeout belongs
    only to compiler-driving harnesses. A target constructing
    ``trybuild::TestCases`` invokes Cargo while its nextest test is running;
    schema-helper fixtures compile with their ordinary integration target and
    are intentionally absent. Cargo names targets after
    ``tests/<name>.rs`` or ``tests/<name>/main.rs``, so both forms are read.

    This is an acquisition function: it searches the directory and reads
    every source it finds. Naming that filesystem access in the docstring
    prevents the query-shaped signature from implying a pure lookup.

    Parameters
    ----------
    tests_directory
        The crate's ``tests`` directory.

    Returns
    -------
    frozenset of str
        Names of test binaries that invoke nested trybuild Cargo.

    Raises
    ------
    SourceReadError
        If the tests directory cannot be searched or a source in it cannot
        be read.
    """
    flat = {
        source.stem
        for source in matching_entries(tests_directory, "*.rs")
        if _drives_the_compiler(source)
    }
    nested = {
        source.parent.name
        for source in matching_entries(tests_directory, "*/main.rs")
        if _drives_the_compiler(source)
    }
    return frozenset(flat | nested)


def _drives_the_compiler(source: Path) -> bool:
    """Return whether one test source constructs ``trybuild::TestCases``.

    Parameters
    ----------
    source
        The root source file for one Cargo test target.

    Returns
    -------
    bool
        True when the target invokes nested Cargo to compile fixtures.

    Raises
    ------
    SourceReadError
        If the source cannot be read.
    """
    return _NESTED_COMPILER_MARKER in read_source(source)
