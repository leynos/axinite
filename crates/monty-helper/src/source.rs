//! Guest source wrapping.
//!
//! The guest supplies an async entrypoint. Two forms are accepted, because
//! the plan states its acceptance example as the complete function while the
//! body form is the shorter one an author writes for a one-liner:
//!
//! - a complete `async def <entrypoint>(params, state): ...`, which is used as
//!   given; or
//! - the body alone, which the helper wraps in the named function.
//!
//! Either way the helper appends the top-level `await`, so every run has the
//! same suspension shape and the guest never controls the module epilogue.

/// The script name shown in guest tracebacks and used for `__file__`.
pub const SCRIPT_NAME: &str = "exec_code.py";

/// Wraps guest source as an async entrypoint and awaits it.
///
/// A body with no statements becomes `pass`, so an empty request still
/// compiles and reports the contract error for its return value.
pub fn wrapper(code: &str, entrypoint: &str) -> String {
    let mut wrapped = String::with_capacity(code.len() + entrypoint.len() + 96);
    if defines_entrypoint(code, entrypoint) {
        for line in code.lines() {
            wrapped.push_str(line);
            wrapped.push('\n');
        }
    } else {
        push_body_form(&mut wrapped, code, entrypoint);
    }
    wrapped.push_str("\nawait ");
    wrapped.push_str(entrypoint);
    wrapped.push_str("(params, state)\n");
    wrapped
}

/// Write `entrypoint` as a function whose body is `code`.
fn push_body_form(wrapped: &mut String, code: &str, entrypoint: &str) {
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
}

/// Report whether `code` already declares the entrypoint function.
///
/// The check is deliberately narrow: the guest may only name an identifier,
/// so a top-level `async def <entrypoint>(` line is unambiguous. A nested
/// definition is indented and so does not match, which keeps a body that
/// happens to define a same-named local from being mistaken for the
/// entrypoint.
fn defines_entrypoint(code: &str, entrypoint: &str) -> bool {
    let declaration = format!("async def {entrypoint}(");
    code.lines()
        .any(|line| !line.starts_with(char::is_whitespace) && line.starts_with(&declaration))
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

    /// The plan's acceptance example passes the complete function, so the
    /// helper must accept it rather than nest it inside a second definition.
    #[test]
    fn accepts_the_plan_acceptance_example() {
        let source = "async def main(params, state):\n    return {\"result\": params[\"value\"], \"state\": state}\n";
        assert_eq!(wrapper(source, "main"), format!("{source}\nawait main(params, state)\n"));
    }

    #[test]
    fn accepts_a_complete_function_without_a_trailing_newline() {
        let source = "async def run(params, state):\n    return {'result': None, 'state': state}";
        assert_eq!(wrapper(source, "run"), format!("{source}\n\nawait run(params, state)\n"));
    }

    /// A body that defines a helper with the entrypoint's name is still a
    /// body: only an unindented declaration at the start of a line counts.
    #[test]
    fn an_indented_definition_of_the_entrypoint_name_is_still_a_body() {
        let source = "    async def main(params, state):\n        pass\n    return main";
        let wrapped = wrapper(source, "main");
        assert!(
            wrapped.starts_with("async def main(params, state):\n        async def main("),
            "{wrapped}"
        );
    }
}
