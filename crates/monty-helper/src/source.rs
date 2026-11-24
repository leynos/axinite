//! Guest source wrapping.
//!
//! The guest supplies only the body of an async entrypoint. The helper wraps
//! it in a named function and a top-level `await`, so every run has the same
//! suspension shape and the guest never controls the module epilogue.

/// The script name shown in guest tracebacks and used for `__file__`.
pub const SCRIPT_NAME: &str = "exec_code.py";

/// Wraps a guest body as an async entrypoint and awaits it.
///
/// A body with no statements becomes `pass`, so an empty request still
/// compiles and reports the contract error for its return value.
pub fn wrapper(code: &str, entrypoint: &str) -> String {
    let mut wrapped = String::with_capacity(code.len() + entrypoint.len() + 96);
    wrapped.push_str("async def ");
    wrapped.push_str(entrypoint);
    wrapped.push_str("(params, state):\n");
    if has_statement(code) {
        for line in code.lines() {
            wrapped.push_str("    ");
            wrapped.push_str(line);
            wrapped.push('\n');
        }
    } else {
        wrapped.push_str("    pass\n");
    }
    wrapped.push_str("\nawait ");
    wrapped.push_str(entrypoint);
    wrapped.push_str("(params, state)\n");
    wrapped
}

fn has_statement(code: &str) -> bool {
    code.lines().any(|line| {
        let trimmed = line.trim_start();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn indents_the_body_and_awaits_the_entrypoint() {
        assert_eq!(
            wrapper("return {'state': state}", "main"),
            "async def main(params, state):\n    return {'state': state}\n\nawait main(params, state)\n"
        );
    }

    #[test]
    fn preserves_relative_indentation() {
        assert_eq!(
            wrapper("if True:\n    return 1", "run"),
            "async def run(params, state):\n    if True:\n        return 1\n\nawait run(params, state)\n"
        );
    }

    #[test]
    fn substitutes_pass_for_an_empty_body() {
        assert_eq!(
            wrapper("", "main"),
            "async def main(params, state):\n    pass\n\nawait main(params, state)\n"
        );
        assert_eq!(
            wrapper("# only a comment\n\n", "main"),
            "async def main(params, state):\n    pass\n\nawait main(params, state)\n"
        );
    }

    #[test]
    fn handles_a_body_without_a_trailing_newline() {
        let wrapped = wrapper("x = 1", "main");
        assert!(wrapped.contains("    x = 1\n"));
        assert!(wrapped.ends_with("await main(params, state)\n"));
    }
}
