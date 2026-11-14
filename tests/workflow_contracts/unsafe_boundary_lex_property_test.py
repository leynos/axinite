"""Generated properties for the Rust lexer used by boundary checks."""

from __future__ import annotations

import re
import sys
import typing as typ
from pathlib import Path

from hypothesis import given
from hypothesis import strategies as st

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "scripts"))

from unsafe_boundaries.rust_lex import (  # noqa: E402
    attribute_bodies,
    strip_rust,
)


@st.composite
def _block_comment(draw: typ.Any) -> str:
    """Generate a nested block comment that mentions `unsafe`."""
    depth = draw(st.integers(min_value=1, max_value=7))
    body = draw(st.text(alphabet="unsafe body\n", max_size=40))
    return "/*" * depth + body + "*/" * depth


@st.composite
def _line_comment(draw: typ.Any) -> str:
    """Generate a line comment that mentions `unsafe`."""
    body = draw(st.text(alphabet="unsafe body / *", max_size=40))
    return f"// unsafe {body}"


@st.composite
def _raw_string(draw: typ.Any) -> str:
    """Generate a raw string whose body mimics an `unsafe` block."""
    hash_count = draw(st.integers(min_value=0, max_value=8))
    hashes = "#" * hash_count
    if hash_count:
        body = 'unsafe { } "' + "#" * (hash_count - 1) + " not the close"
    else:
        body = "unsafe { } # not the close"
    return f'r{hashes}"{body}"{hashes}'


@st.composite
def _escaped_string(draw: typ.Any) -> str:
    """Generate a string with escapes that mentions `unsafe`."""
    body = draw(st.text(alphabet="unsafe body {}[]", max_size=40))
    return r'"unsafe \" ' + body + r'\n"'


def _char_literal() -> st.SearchStrategy[str]:
    """Generate representative character literals, including escaped forms."""
    return st.sampled_from(("'a'", "'_'", r"'\n'", r"'\''", r"'\\'"))


IGNORED_SPANS = st.one_of(
    _block_comment(),
    _line_comment(),
    _raw_string(),
    _escaped_string(),
    _char_literal(),
)


def _newline_positions(text: str) -> list[int]:
    """Return the index of every newline in `text`."""
    return [index for index, character in enumerate(text) if character == "\n"]


@given(
    noise=st.lists(IGNORED_SPANS, min_size=1, max_size=10).map(
        lambda spans: "\n".join(spans)
    )
)
def test_generated_comments_and_literals_are_blank_without_shifting_offsets(
    noise: str,
) -> None:
    """Blanked spans keep their length and every newline in place."""
    stripped = strip_rust(noise)

    assert len(stripped) == len(noise)
    assert _newline_positions(stripped) == _newline_positions(noise)
    assert all(character in " \n" for character in stripped)


@given(
    line=_line_comment(),
    raw=_raw_string(),
    escaped=_escaped_string(),
    character=_char_literal(),
)
def test_keep_literals_preserves_literals_but_still_blanks_comments(
    line: str,
    raw: str,
    escaped: str,
    character: str,
) -> None:
    """Literal contents survive while comment text is still blanked."""
    source = "\n".join((line, raw, escaped, character))
    stripped = strip_rust(source, keep_literals=True)

    assert len(stripped) == len(source)
    assert _newline_positions(stripped) == _newline_positions(source)
    assert line not in stripped
    assert raw in stripped
    assert escaped in stripped
    assert character in stripped


@given(
    noise=st.lists(IGNORED_SPANS, min_size=1, max_size=8),
    lifetime=st.from_regex(r"'[a-z][a-z0-9_]{0,7}", fullmatch=True),
)
def test_real_unsafe_after_generated_noise_is_still_visible(
    noise: list[str],
    lifetime: str,
) -> None:
    """A real `unsafe` block stays visible after arbitrary lexer noise."""
    source = "\n".join(noise)
    source += f"\nfn borrow<{lifetime}>(value: &{lifetime} str) {{ unsafe {{}} }}"
    stripped = strip_rust(source)

    assert len(stripped) == len(source)
    assert _newline_positions(stripped) == _newline_positions(source)
    assert stripped.count("unsafe") == 1
    assert lifetime in stripped
    assert re.search(r"\bunsafe\s*\{", stripped)


@given(
    depth=st.integers(min_value=0, max_value=8),
    note=st.text(alphabet="abc xyz", max_size=12),
    prefix=st.sampled_from(("#", "#!")),
)
def test_attribute_bodies_balance_generated_nested_brackets(
    depth: int,
    note: str,
    prefix: str,
) -> None:
    """Attribute bodies expose balanced generated brackets, not string contents."""
    nested = "value"
    for _ in range(depth):
        nested = f"level([{nested}])"
    source = f'{prefix}[cfg_attr(all({nested}), note = "[{note}]")] fn f() {{}}'
    bodies = list(attribute_bodies(strip_rust(source)))

    assert len(bodies) == 1
    assert nested in bodies[0]
    assert "note =" in bodies[0]
    assert f"[{note}]" not in bodies[0]
