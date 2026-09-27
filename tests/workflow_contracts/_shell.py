"""Where one shell command ends and the next begins.

A workflow step's `run:` block is a shell script, and the readers that ask
what a step executes match a command name and then take its arguments. Taking
them to the end of the physical line is what this module exists to stop:
`cargo nextest run --workspace && cargo test --features harness` is two
commands, and reading the line as one gives the first command the second's
feature flag. The duplicate key that results describes neither run.

The split is deliberately small. It respects single and double quotes, and it
breaks on the unquoted operators that end a command: `;`, `&&`, `||`, `|`,
`&`, and a newline. It does not understand subshells, here-documents,
backslash continuations or variable expansion, because the estate's steps do
not use them to run a suite, and a reader that guessed at those would be
confidently wrong rather than cautiously narrow.

Narrow is the safe direction. A command this splits too eagerly is read with
fewer arguments than it has, which keys it as a different run; a command it
fails to split is read with another command's arguments, which keys two
different runs as one and reports a duplicate where there are two suites.
"""

from __future__ import annotations

#: The unquoted characters that can end a command. `&` and `|` cover their
#: doubled forms too: the split is on the first of the pair, and the second
#: is left at the head of the next piece, where no command name matches it.
SEPARATORS = frozenset({";", "&", "|", "\n"})

#: The quote characters that suspend the split. A separator inside either is
#: an ordinary character, so `--filter 'a|b'` stays one command.
QUOTES = frozenset({"'", '"'})


def split_commands(script: str) -> list[str]:
    """Return the shell commands a script runs, in order.

    Parameters
    ----------
    script
        A step's `run:` block, with every matrix reference already
        substituted.

    Returns
    -------
    list of str
        Each command's text, stripped, with empty pieces dropped. A script
        that runs one command yields one entry, so a caller can iterate
        unconditionally.

    Examples
    --------
    >>> split_commands("cargo nextest run --workspace && cargo test -F x")
    ['cargo nextest run --workspace', 'cargo test -F x']
    >>> split_commands("cargo test -E 'test(a|b)'")
    ["cargo test -E 'test(a|b)'"]
    """
    commands: list[str] = []
    current: list[str] = []
    quote: str | None = None
    for character in script:
        if quote is not None:
            current.append(character)
            if character == quote:
                quote = None
            continue
        if character in QUOTES:
            quote = character
            current.append(character)
            continue
        if character in SEPARATORS:
            commands.append("".join(current))
            current = []
            continue
        current.append(character)
    commands.append("".join(current))
    return [command.strip() for command in commands if command.strip()]
