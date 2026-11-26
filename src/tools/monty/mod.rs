//! Bounded Python execution through the isolated Monty helper.
//!
//! The parent never links the interpreter. It resolves a run's tools into an
//! immutable capability table, starts one helper process per run, and mediates
//! every host callback through the caller's ordinary policy path. See
//! `docs/adrs/adr-0016-monty-execution.md`.
//!
//! Modules:
//!
//! - [`capabilities`] resolves and pins the selected tools.
//! - [`broker`] answers callbacks from the selected table.
//! - [`runner`] supervises the helper process.
//! - [`exec_code`] exposes the public tool.

pub mod broker;
pub mod capabilities;
pub mod exec_code;
pub mod runner;

/// Python keywords that cannot be used as a tool or entrypoint name.
///
/// The generated wrapper interpolates names into Python source, so a keyword
/// would produce a syntax error rather than a callable function.
const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// Whether `name` is a non-keyword ASCII Python identifier.
///
/// The subset is deliberately narrower than Python's: identifiers are ASCII,
/// start with a letter or underscore, and continue with alphanumerics or
/// underscores. A name outside this subset is rejected rather than escaped
/// into generated source.
pub fn is_python_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !characters.all(|character| character.is_ascii_alphanumeric() || character == '_') {
        return false;
    }
    !PYTHON_KEYWORDS.contains(&name)
}

#[cfg(test)]
mod tests {
    //! Unit tests for the Monty module wiring.

    use super::*;

    #[test]
    fn accepts_ordinary_identifiers() {
        for name in ["lookup", "lookup_2", "_private", "A1"] {
            assert!(is_python_identifier(name), "{name} should be accepted");
        }
    }

    #[test]
    fn rejects_keywords_and_non_identifiers() {
        for name in ["class", "None", "2fast", "has-dash", "has.dot", "", "café"] {
            assert!(!is_python_identifier(name), "{name} should be rejected");
        }
    }
}
