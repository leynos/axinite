"""Search generated Rust harnesses for lexical masking regressions."""

from __future__ import annotations

from collections.abc import Sequence
from pathlib import Path
from string import ascii_letters, ascii_lowercase, digits
from tempfile import TemporaryDirectory

from hypothesis import given, strategies as st

from schema_helper_fixtures import (
    _rust_lexical_spans,
    _rust_source_views,
    read_fixture_inventory,
)

_FIXTURES = (
    ("param_name_call", "param_name_call.rs"),
    ("str_call", "str_call.rs"),
)
_LEXICAL_PARTS = ("string", "line_comment", "block_comment", "ordinary_code")
_CALL_PARTS = tuple(f"call_{module}" for module, _ in _FIXTURES)
_PAYLOAD_ALPHABET = ascii_letters + digits + " _/\\\"!;:(){}*#-"
_COMMENT_ALPHABET = ascii_letters + digits + " _-.:;"


def _rust_string_literal(value: str) -> str:
    """Render an ordinary Rust string with escaped quotes and line continuations."""
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    escaped = escaped.replace("\n", "\\\n")
    return f'"{escaped}"'


def _fake_rust_syntax(decoy: str) -> str:
    """Render registration, invocation, UI-test, and trybuild decoys."""
    return (
        f"mod {decoy} {{\n"
        f'    include!("pass/{decoy}.rs");\n'
        "    pub(super) fn run() {\n"
        "        main();\n"
        "    }\n"
        "}\n"
        "#[test]\n"
        "fn ui() {\n"
        f"    {decoy}::run();\n"
        "}\n"
        "trybuild::TestCases::new();"
    )


def _string_decoy(fake_syntax: str, payload: str) -> str:
    """Put multiline fake harness syntax in an escaped ordinary string."""
    quoted_payload = f"\n{fake_syntax}\n{payload} \"quoted\" \\\\ tail"
    return "    let _quoted_source = " + _rust_string_literal(quoted_payload) + ";"


def _line_comment_decoy(fake_syntax: str, filler: str) -> str:
    """Comment every generated decoy line so calls cannot escape the comment."""
    lines = fake_syntax.splitlines()
    lines[-1] += f" {filler}"
    return "\n".join(
        f"    // {line}" if index == 0 else f"// {line}"
        for index, line in enumerate(lines)
    )


def _nested_block_comment_decoy(
    fake_syntax: str,
    filler: str,
    depth: int,
) -> str:
    """Wrap syntax in a balanced generated depth of nested block comments."""
    return (
        "    /* outer\n"
        + "/* inner\n" * (depth - 1)
        + fake_syntax
        + f"\n{filler}\n"
        + "*/\n" * depth
    )


def _render_harness(fragments: dict[str, str], order: Sequence[str]) -> str:
    """Place registrations and ordered code fragments in one valid UI harness."""
    modules = "\n".join(
        f"mod {module} {{\n"
        f'    include!("pass/{fixture}");\n'
        "    pub(super) fn run() {\n"
        "        main();\n"
        "    }\n"
        "}"
        for module, fixture in _FIXTURES
    )
    body = "\n".join(fragments[part] for part in order)
    return f"{modules}\n\n#[test]\nfn ui() {{\n{body}\n}}\n"


@st.composite
def _generated_harness(draw: st.DrawFn) -> tuple[str, tuple[str, ...]]:
    """Build a bounded valid harness with generated lexical tokens and calls.

    The ordinary string uses Rust line continuations to represent multiline
    decoy source. Comments, integer statements, and the real wrapper calls are
    interleaved as valid items inside the generated ``ui`` function.
    """
    suffix = draw(st.text(alphabet=ascii_lowercase, min_size=1, max_size=8))
    payload = draw(st.text(alphabet=_PAYLOAD_ALPHABET, max_size=32))
    comment_filler = draw(st.text(alphabet=_COMMENT_ALPHABET, max_size=24))
    block_depth = draw(st.integers(min_value=1, max_value=5))
    ordinary_value = draw(st.integers(min_value=0, max_value=10_000))
    order = draw(st.permutations((*_LEXICAL_PARTS, *_CALL_PARTS)))

    decoy = f"ghost_{suffix}"
    fake_syntax = _fake_rust_syntax(decoy)
    fragments = {
        "string": _string_decoy(fake_syntax, payload),
        "line_comment": _line_comment_decoy(fake_syntax, comment_filler),
        "block_comment": _nested_block_comment_decoy(
            fake_syntax,
            comment_filler,
            block_depth,
        ),
        "ordinary_code": f"    let _value_{suffix}: usize = {ordinary_value};",
    }
    calls = {
        f"call_{module}": (module, f"    {module}::run();")
        for module, _ in _FIXTURES
    }
    fragments.update({part: rendered for part, (_, rendered) in calls.items()})
    expected_invocations = tuple(
        calls[part][0] for part in order if part in calls
    )
    return _render_harness(fragments, order), expected_invocations


def _line_breaks(source: str) -> tuple[tuple[int, str], ...]:
    """Return line-break characters with their source offsets."""
    return tuple(
        (index, character)
        for index, character in enumerate(source)
        if character in "\r\n"
    )


def _is_blank_except_line_breaks(source: str) -> bool:
    """Report whether a masked span contains only spaces and line breaks."""
    return all(character in " \r\n" for character in source)


@given(case=_generated_harness())
def test_generated_rust_views_preserve_lexical_geometry(
    case: tuple[str, tuple[str, ...]],
) -> None:
    """Masking generated Rust tokens preserves source and span geometry."""
    source, _ = case
    spans = _rust_lexical_spans(source)
    comment_free, code_only = _rust_source_views(source)

    assert spans.strings == tuple(sorted(spans.strings)), (
        "ordinary-string spans must stay in source order"
    )
    assert spans.comments == tuple(sorted(spans.comments)), (
        "comment spans must stay in source order"
    )
    all_spans = sorted((*spans.strings, *spans.comments))
    assert all(0 <= start < end <= len(source) for start, end in all_spans), (
        "lexical spans must be non-empty and stay within the source"
    )
    assert all(
        previous_end <= next_start
        for (_, previous_end), (next_start, _) in zip(all_spans, all_spans[1:])
    ), "ordinary-string and comment spans must not overlap"

    assert len(comment_free) == len(source), (
        "comment masking must preserve source length"
    )
    assert len(code_only) == len(source), "full masking must preserve source length"
    assert _line_breaks(comment_free) == _line_breaks(source), (
        "comment masking must preserve newline characters and offsets"
    )
    assert _line_breaks(code_only) == _line_breaks(source), (
        "full masking must preserve newline characters and offsets"
    )

    assert all(
        _is_blank_except_line_breaks(comment_free[start:end])
        and _is_blank_except_line_breaks(code_only[start:end])
        for start, end in spans.comments
    ), "both source views must blank every generated comment"
    assert all(
        comment_free[start:end] == source[start:end]
        and _is_blank_except_line_breaks(code_only[start:end])
        for start, end in spans.strings
    ), "only the code-only view may blank ordinary strings"


@given(case=_generated_harness())
def test_generated_lexical_decoys_do_not_change_fixture_inventory(
    case: tuple[str, tuple[str, ...]],
) -> None:
    """Strings and comments cannot add fixtures or replace live UI calls."""
    source, expected_invocations = case
    with TemporaryDirectory() as temporary_directory:
        tests_directory = Path(temporary_directory)
        pass_directory = tests_directory / "pass"
        pass_directory.mkdir()
        for _, fixture in _FIXTURES:
            (pass_directory / fixture).write_text("fn main() {}\n", encoding="utf-8")
        (tests_directory / "main.rs").write_text(source, encoding="utf-8")

        inventory = read_fixture_inventory(tests_directory)

    expected_modules = dict(_FIXTURES)
    expected_modules_in_order = tuple(_FIXTURES)
    assert inventory.discovered == frozenset(expected_modules.values()), (
        "only the two generated pass fixtures should be discovered"
    )
    assert inventory.registrations == expected_modules_in_order, (
        "decoys must not register modules or suppress the live registrations"
    )
    assert inventory.invocations == expected_invocations, (
        "decoy calls must not alter the live UI invocation order"
    )
    assert not inventory.invalid_modules, "decoys must not create invalid modules"
    assert not inventory.outside_includes, "decoy includes must not escape pass"
    assert not inventory.uses_trybuild, "decoy trybuild syntax must stay masked"
