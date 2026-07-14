//! Tests for `Language` file extensions and build/test command planning.

use std::path::Path;

use super::super::Language;

#[test]
fn test_language_extension_all_variants() {
    assert_eq!(Language::Rust.extension(), "rs");
    assert_eq!(Language::Python.extension(), "py");
    assert_eq!(Language::TypeScript.extension(), "ts");
    assert_eq!(Language::JavaScript.extension(), "js");
    assert_eq!(Language::Go.extension(), "go");
    assert_eq!(Language::Bash.extension(), "sh");
}

#[test]
fn test_language_build_command_compiled_returns_some() {
    let dir = Path::new("/tmp/project");
    let rust_cmd = Language::Rust.build_command(dir);
    assert!(rust_cmd.is_some());
    let rust_cmd = rust_cmd.expect("rust build command");
    assert_eq!(rust_cmd.program, "cargo");
    assert_eq!(rust_cmd.args, vec!["build", "--release"]);

    let ts_cmd = Language::TypeScript.build_command(dir);
    assert!(ts_cmd.is_some());
    let ts_cmd = ts_cmd.expect("typescript build command");
    assert_eq!(ts_cmd.program, "npm");
    assert_eq!(ts_cmd.args, vec!["run", "build"]);

    let go_cmd = Language::Go.build_command(dir);
    assert!(go_cmd.is_some());
    let go_cmd = go_cmd.expect("go build command");
    assert_eq!(go_cmd.program, "go");
    assert_eq!(go_cmd.args, vec!["build", "./..."]);
}

#[test]
fn test_language_build_command_interpreted_returns_none() {
    let dir = Path::new("/tmp/project");
    assert!(Language::Python.build_command(dir).is_none());
    assert!(Language::JavaScript.build_command(dir).is_none());
    assert!(Language::Bash.build_command(dir).is_none());
}

#[test]
fn test_language_build_command_includes_project_dir() {
    let dir = Path::new("/home/user/my_project");
    for lang in [Language::Rust, Language::TypeScript, Language::Go] {
        let cmd = lang.build_command(dir);
        assert!(
            cmd.as_ref()
                .expect("compiled language build command")
                .cwd
                .as_path()
                == dir,
            "{:?} build command should contain project dir",
            lang
        );
    }
}

#[test]
fn test_language_test_command_all_variants_non_empty() {
    let dir = Path::new("/tmp/project");
    let all_languages = [
        Language::Rust,
        Language::Python,
        Language::TypeScript,
        Language::JavaScript,
        Language::Go,
        Language::Bash,
    ];
    for lang in all_languages {
        let cmd = lang.test_command(dir);
        assert!(
            !cmd.program.is_empty(),
            "{:?} test command should not be empty",
            lang
        );
        assert!(
            cmd.cwd.as_path() == dir,
            "{:?} test command should contain project dir",
            lang
        );
    }
}

#[test]
fn test_language_test_command_specific_tools() {
    let dir = Path::new("/tmp/p");
    assert_eq!(Language::Rust.test_command(dir).program, "cargo");
    assert_eq!(
        Language::Python.test_command(dir).args,
        vec!["-m", "pytest"]
    );
    assert_eq!(Language::TypeScript.test_command(dir).program, "npm");
    assert_eq!(Language::JavaScript.test_command(dir).program, "npm");
    assert_eq!(Language::Go.test_command(dir).args, vec!["test", "./..."]);
    assert_eq!(Language::Bash.test_command(dir).program, "sh");
}
