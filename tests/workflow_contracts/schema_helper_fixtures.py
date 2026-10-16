"""Read the schema-helper fixture inventory and its integration-test calls."""

from __future__ import annotations

import re
from collections import Counter
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Final, Literal, TypeAlias

from contract_sources import matching_entries, read_source

_MODULE = re.compile(r"(?ms)^mod (?P<name>[A-Za-z_]\w*)\s*\{(?P<body>.*?)^\}")
_INCLUDE = re.compile(r'\binclude!\s*\(\s*"(?P<path>[^"]+)"\s*\)\s*;')
_RUN_WRAPPER = re.compile(
    r"\bpub\s*\(\s*super\s*\)\s+fn\s+run\s*\(\s*\)\s*"
    r"\{\s*main\s*\(\s*\)\s*;\s*\}"
)
_UI_TEST = re.compile(
    r"(?ms)^#\[test\]\s*\nfn ui\s*\(\s*\)\s*\{(?P<body>.*?)^\}"
)
_RUN_CALL = re.compile(r"\b(?P<module>[A-Za-z_]\w*)::run\s*\(\s*\)\s*;")
_TRYBUILD_USE = re.compile(r"\btrybuild\s*::|\bTestCases\s*::")
_LEXICAL_STARTER = re.compile(r'"|//|/\*')

_LexicalBucket: TypeAlias = Literal["strings", "comments"]
_BoundaryFinder: TypeAlias = Callable[[str, int], int]


@dataclass(frozen=True, slots=True)
class RustLexicalSpans:
    """Non-overlapping ordinary-string and comment spans in Rust source."""

    strings: tuple[tuple[int, int], ...]
    comments: tuple[tuple[int, int], ...]


def _render_rust_source_view(
    source: str,
    spans: RustLexicalSpans,
    *,
    mask_strings: bool,
) -> str:
    """Render source with comments and, optionally, string literals blanked."""
    rendered = list(source)
    masked_spans = spans.comments + (spans.strings if mask_strings else ())
    for start, end in masked_spans:
        _blank_non_newlines(rendered, start, end)
    return "".join(rendered)


def _rust_source_views(source: str) -> tuple[str, str]:
    """Return comment-free source and code with ordinary strings masked.

    The first view keeps include paths intact, while the second prevents
    quoted text from posing as Rust syntax.
    """
    spans = _rust_lexical_spans(source)
    return (
        _render_rust_source_view(source, spans, mask_strings=False),
        _render_rust_source_view(source, spans, mask_strings=True),
    )


def _quoted_string_end(source: str, start: int) -> int:
    """Return the offset after an ordinary string, including escaped quotes."""
    index = start + 1
    while index < len(source):
        if source[index] == "\\":
            index += 2
        elif source[index] == '"':
            return index + 1
        else:
            index += 1
    return len(source)


def _line_comment_end(source: str, start: int) -> int:
    """Return the offset before a line-comment newline or at source end."""
    newline = source.find("\n", start)
    return len(source) if newline < 0 else newline


def _block_comment_end(source: str, start: int) -> int:
    """Return the offset after a nested block comment or the source end."""
    depth = 1
    index = start + 2
    while index < len(source) and depth:
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    return index


_LEXICAL_TOKEN_DISPATCH: Final[
    dict[str, tuple[_LexicalBucket, _BoundaryFinder]]
] = {
    '"': ("strings", _quoted_string_end),
    "//": ("comments", _line_comment_end),
    "/*": ("comments", _block_comment_end),
}


def _rust_lexical_spans(source: str) -> RustLexicalSpans:
    """Discover ordinary strings and comments in the controlled harness source.

    Each boundary finder consumes a complete token, so comment markers inside
    strings and nested comments are not reconsidered as lexical starters.
    """
    spans: dict[_LexicalBucket, list[tuple[int, int]]] = {
        "strings": [],
        "comments": [],
    }
    cursor = 0
    while (token := _LEXICAL_STARTER.search(source, cursor)) is not None:
        start = token.start()
        bucket, find_end = _LEXICAL_TOKEN_DISPATCH[token.group()]
        end = find_end(source, start)
        spans[bucket].append((start, end))
        cursor = end
    return RustLexicalSpans(
        strings=tuple(spans["strings"]),
        comments=tuple(spans["comments"]),
    )


def _blank_non_newlines(source: list[str], start: int, end: int) -> None:
    """Replace a source span with spaces while preserving its line layout."""
    for index in range(start, end):
        if source[index] not in "\r\n":
            source[index] = " "


def _include_matches(comment_free: str, code_only: str) -> tuple[re.Match[str], ...]:
    """Find real include macros while retaining their quoted fixture paths."""
    return tuple(
        match
        for match in _INCLUDE.finditer(comment_free)
        if code_only[match.start() : match.start() + len("include!")] == "include!"
    )


@dataclass(frozen=True)
class FixtureInventory:
    """Coverage evidence for direct ``pass/*.rs`` fixture files."""

    discovered: frozenset[str]
    registrations: tuple[tuple[str, str], ...]
    invocations: tuple[str, ...]
    invalid_modules: tuple[str, ...]
    outside_includes: tuple[str, ...]
    has_ui_test: bool
    uses_trybuild: bool

    @property
    def registered(self) -> frozenset[str]:
        """Fixture filenames with a valid module-local execution wrapper."""
        return frozenset(fixture for _, fixture in self.registrations)

    @property
    def unregistered(self) -> frozenset[str]:
        """Discovered fixture files that the harness does not register."""
        return self.discovered - self.registered

    @property
    def missing_files(self) -> frozenset[str]:
        """Harness registrations whose fixture file no longer exists."""
        return self.registered - self.discovered

    @property
    def not_executed(self) -> frozenset[str]:
        """Registered fixtures whose module wrapper is not called by ``ui``."""
        module_fixtures = dict(self.registrations)
        invoked_fixtures = {
            module_fixtures[module]
            for module in self.invocations
            if module in module_fixtures
        }
        return self.registered - invoked_fixtures

    @property
    def duplicate_registrations(self) -> frozenset[str]:
        """Fixtures included by more than one wrapper module."""
        counts = Counter(fixture for _, fixture in self.registrations)
        return frozenset(name for name, count in counts.items() if count > 1)

    @property
    def duplicate_invocations(self) -> frozenset[str]:
        """Wrapper modules called more than once by ``ui``."""
        counts = Counter(self.invocations)
        return frozenset(name for name, count in counts.items() if count > 1)

    @property
    def unknown_invocations(self) -> frozenset[str]:
        """Wrapper calls that do not name a registered module."""
        modules = {module for module, _ in self.registrations}
        return frozenset(self.invocations) - modules


@dataclass(frozen=True, slots=True)
class ModuleRegistrationInventory:
    """Registration evidence collected from the harness module declarations."""

    registrations: tuple[tuple[str, str], ...]
    invalid_modules: tuple[str, ...]
    outside_includes: tuple[str, ...]


def _collect_module_registrations(
    comment_free: str,
    code_only: str,
) -> ModuleRegistrationInventory:
    """Collect valid fixture wrappers and malformed or misplaced includes."""
    include_matches = _include_matches(comment_free, code_only)
    registrations: list[tuple[str, str]] = []
    invalid_modules: list[str] = []
    module_include_spans: set[tuple[int, int]] = set()

    for module in _MODULE.finditer(code_only):
        body = module.group("body")
        body_start = module.start("body")
        body_end = module.end("body")
        includes = [
            match
            for match in include_matches
            if body_start <= match.start() < body_end
        ]
        if not includes:
            continue
        module_include_spans.update(
            (match.start(), match.end())
            for match in includes
        )
        fixture = _fixture_for_module(body, includes)
        if fixture is None:
            invalid_modules.append(module.group("name"))
            continue
        registrations.append((module.group("name"), fixture))

    outside_includes = tuple(
        match.group("path")
        for match in include_matches
        if (match.start(), match.end()) not in module_include_spans
    )

    return ModuleRegistrationInventory(
        registrations=tuple(registrations),
        invalid_modules=tuple(invalid_modules),
        outside_includes=outside_includes,
    )


def _fixture_for_module(body: str, includes: Sequence[re.Match[str]]) -> str | None:
    """Derive a fixture filename from one supported module binding shape."""
    if len(includes) != 1:
        return None
    fixture_path = includes[0].group("path")
    if re.fullmatch(r"pass/[^/]+\.rs", fixture_path) is None:
        return None
    if _RUN_WRAPPER.search(body) is None:
        return None
    return Path(fixture_path).name


def read_fixture_inventory(tests_directory: Path) -> FixtureInventory:
    """Read pass-fixture files and verify their harness registration shape.

    The contract deliberately accepts one simple pattern: each top-level
    module includes one ``pass/*.rs`` fixture and exposes ``run`` which calls
    that fixture's ``main``; the ``ui`` test calls each wrapper once. Line and
    block comments are removed, and ordinary strings are masked when checking
    code so prose cannot register or execute a fixture.

    Raises
    ------
    SourceReadError
        If the pass directory cannot be listed or ``main.rs`` cannot be read.
    """
    pass_directory = tests_directory / "pass"
    discovered = frozenset(
        path.name for path in matching_entries(pass_directory, "*.rs")
    )
    source = read_source(tests_directory / "main.rs")
    comment_free, code_only = _rust_source_views(source)
    module_inventory = _collect_module_registrations(comment_free, code_only)
    ui_test = _UI_TEST.search(code_only)
    invocations = (
        tuple(
            match.group("module")
            for match in _RUN_CALL.finditer(ui_test.group("body"))
        )
        if ui_test is not None
        else ()
    )

    return FixtureInventory(
        discovered=discovered,
        registrations=module_inventory.registrations,
        invocations=invocations,
        invalid_modules=module_inventory.invalid_modules,
        outside_includes=module_inventory.outside_includes,
        has_ui_test=ui_test is not None,
        uses_trybuild=_TRYBUILD_USE.search(code_only) is not None,
    )
