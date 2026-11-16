"""Checks on the generated-bindings module of a boundary crate."""

from __future__ import annotations

import re
from pathlib import Path

from .files import read_text
from .inventory import BINDINGS_DIRECTORY, BINDINGS_FILE
from .rust_lex import ABI_ATTRIBUTES, attribute_bodies, strip_rust

#: The statements the bindings file must contain once each: a `use super::X;`
#: naming the guest type, the generate! invocation, and the export! invocation.
_BINDINGS_STATEMENTS = (
    re.compile(r"use\s+super::(?P<type>\w+)\s*;"),
    re.compile(r"wit_bindgen::generate!\(\{[^;]*\}\)\s*;"),
    re.compile(r"export!\(\s*(?P<export>\w+)\s*\)\s*;"),
)


#: A test-only module of Miri round trips through the generated glue. It lives
#: in `src/bindings/` so it shares the bindings module's `expect`, and it may
#: hold `unsafe` because it plays the host calling the exported shims.
_ABI_TEST_MODULE = re.compile(
    r"#\[cfg\(all\(test,\s*target_pointer_width\s*=\s*\"32\"\)\)\]"
    r"\s*mod\s+(?P<module>\w+)\s*;"
)


def declared_abi_test_modules(crate_dir: Path) -> set[str]:
    """Return the `src/bindings/*.rs` files declared as 32-bit test modules."""
    stripped = strip_rust(read_text(crate_dir / BINDINGS_FILE), keep_literals=True)
    return {
        f"{BINDINGS_DIRECTORY}{match.group('module')}.rs"
        for match in _ABI_TEST_MODULE.finditer(stripped)
    }


def check_bindings_file(path: Path) -> list[str]:
    """Require the bindings file to hold only generated-binding invocations."""
    remainder = strip_rust(read_text(path), keep_literals=True)
    found: dict[str, str] = {}
    for pattern in _BINDINGS_STATEMENTS:
        matches = list(pattern.finditer(remainder))
        if len(matches) != 1:
            return [
                f"{BINDINGS_FILE}: expected exactly one `{pattern.pattern}` statement"
            ]
        found.update({k: v for k, v in matches[0].groupdict().items() if v})
        remainder = remainder[: matches[0].start()] + remainder[matches[0].end() :]
    remainder = _ABI_TEST_MODULE.sub("", remainder)
    problems = []
    if remainder.strip():
        problems.append(f"{BINDINGS_FILE}: handwritten content beyond the bindings")
    if found.get("type") != found.get("export"):
        problems.append(
            f"{BINDINGS_FILE}: export! must name the type imported from super"
        )
    if re.search(r"\bunsafe\b", strip_rust(remainder)):
        problems.append(f"{BINDINGS_FILE}: handwritten `unsafe`")
    return problems


def check_abi_test_module(relative: str, text: str) -> list[str]:
    """Allow `unsafe` in an ABI test module, but nothing that exports or relaxes."""
    problems = []
    for body in attribute_bodies(strip_rust(text)):
        if ABI_ATTRIBUTES.search(body):
            problems.append(
                f"{relative}: ABI attribute in a test module: #[{body.strip()}]"
            )
        if "unsafe_code" in body:
            problems.append(f"{relative}: unsafe_code lint override: #[{body.strip()}]")
    return problems
