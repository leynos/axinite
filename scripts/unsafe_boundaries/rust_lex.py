"""Remove comments and literals from Rust source before token searches."""

from __future__ import annotations

import re
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator


def _blank(text: str, start: int, end: int) -> str:
    """Return `text[start:end]` with everything but newlines replaced by spaces."""
    return "".join(ch if ch == "\n" else " " for ch in text[start:end])


def _raw_string_end(text: str, index: int) -> int | None:
    """Return the end of a raw string literal starting at `index`, if any."""
    match = re.compile(r'b?r(#*)"').match(text, index)
    if match is None:
        return None
    closing = '"' + match.group(1)
    end = text.find(closing, match.end())
    return len(text) if end == -1 else end + len(closing)


def _string_end(text: str, index: int) -> int:
    """Return the end of an ordinary string literal whose quote is at `index`."""
    cursor = index + 1
    while cursor < len(text):
        if text[cursor] == "\\":
            cursor += 2
        elif text[cursor] == '"':
            return cursor + 1
        else:
            cursor += 1
    return len(text)


def _char_literal_end(text: str, index: int) -> int | None:
    """Return the end of a char literal at `index`; `None` for a lifetime."""
    if text.startswith("\\", index + 1):
        end = text.find("'", index + 3)
        return None if end == -1 else end + 1
    if text.startswith("'", index + 2):
        return index + 3
    return None


_BLOCK_COMMENT_TOKEN = re.compile(r"/\*|\*/")


def _block_comment_end(text: str, index: int) -> int:
    """Return the end of a (nested) block comment starting at `index`."""
    depth = 0
    for token in _BLOCK_COMMENT_TOKEN.finditer(text, index):
        depth += 1 if token.group() == "/*" else -1
        if depth == 0:
            return token.end()
    return len(text)


def strip_rust(text: str, *, keep_literals: bool = False) -> str:
    """Blank comments, and string and char literals, from Rust source.

    Line structure is preserved so diagnostics keep their line numbers.

    Parameters
    ----------
    text
        Rust source text.
    keep_literals
        Keep string and char literals, blanking comments only. Used to read an
        attribute's `reason` string without being fooled by commented-out code.

    Returns
    -------
    str
        The text with the selected noise replaced by spaces.
    """
    out: list[str] = []
    cursor = 0
    while cursor < len(text):
        end, blank = _next_span(text, cursor, keep_literals=keep_literals)
        out.append(_blank(text, cursor, end) if blank else text[cursor:end])
        cursor = end
    return "".join(out)


def _next_span(text: str, index: int, *, keep_literals: bool) -> tuple[int, bool]:
    """Return `(end, blank)` for the span of text starting at `index`."""
    if text.startswith("//", index):
        newline = text.find("\n", index)
        return (len(text) if newline == -1 else newline), True
    if text.startswith("/*", index):
        return _block_comment_end(text, index), True
    raw_end = _raw_string_end(text, index)
    if raw_end is not None and not _is_identifier_tail(text, index):
        return raw_end, not keep_literals
    if text[index] == '"':
        return _string_end(text, index), not keep_literals
    if text[index] == "'":
        char_end = _char_literal_end(text, index)
        if char_end is not None:
            return char_end, not keep_literals
    return index + 1, False


def _is_identifier_tail(text: str, index: int) -> bool:
    """Report whether `index` continues an identifier (so `r"` is not raw)."""
    return index > 0 and (text[index - 1].isalnum() or text[index - 1] == "_")


ABI_ATTRIBUTES = re.compile(r"\b(?:no_mangle|export_name|link_section)\b")


def attribute_bodies(stripped: str) -> Iterator[str]:
    """Yield the text inside every `#[...]` and `#![...]` attribute."""
    for match in re.finditer(r"#\s*!?\s*\[", stripped):
        depth = 1
        cursor = match.end()
        while cursor < len(stripped) and depth:
            depth += {"[": 1, "]": -1}.get(stripped[cursor], 0)
            cursor += 1
        yield stripped[match.end() : cursor - 1]
