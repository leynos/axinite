//! Tests for `Language` file extensions and build/test command planning.

use std::path::Path;

use pretty_assertions::assert_eq;
use rstest::rstest;

use super::super::Language;

#[rstest]
#[case(Language::Rust, "rs")]
#[case(Language::Python, "py")]
#[case(Language::TypeScript, "ts")]
#[case(Language::JavaScript, "js")]
#[case(Language::Go, "go")]
#[case(Language::Bash, "sh")]
fn test_language_extension_all_variants(#[case] language: Language, #[case] expected_ext: &str) {
    assert_eq!(language.extension(), expected_ext);
}

#[rstest]
#[case(Language::Rust, "cargo", vec!["build", "--release"])]
#[case(Language::TypeScript, "npm", vec!["run", "build"])]
#[case(Language::Go, "go", vec!["build", "./..."])]
fn test_language_build_command_compiled_returns_some(
    #[case] language: Language,
    #[case] expected_program: &str,
    #[case] expected_args: Vec<&str>,
) {
    let dir = Path::new("/tmp/project");
    let cmd = language.build_command(dir);
    assert!(cmd.is_some());
    let cmd = cmd.expect("compiled language build command");
    assert_eq!(cmd.program, expected_program);
    assert_eq!(cmd.args, expected_args);
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

#[rstest]
#[case(Language::Rust, "cargo", vec!["test"])]
#[case(Language::Python, "python", vec!["-m", "pytest"])]
#[case(Language::TypeScript, "npm", vec!["test"])]
#[case(Language::JavaScript, "npm", vec!["test"])]
#[case(Language::Go, "go", vec!["test", "./..."])]
#[case(Language::Bash, "sh", vec!["-c", "shellcheck *.sh"])]
fn test_language_test_command_specific_tools(
    #[case] language: Language,
    #[case] expected_program: &str,
    #[case] expected_args: Vec<&str>,
) {
    let dir = Path::new("/tmp/p");
    let cmd = language.test_command(dir);
    assert_eq!(cmd.program, expected_program);
    assert_eq!(cmd.args, expected_args);
}
