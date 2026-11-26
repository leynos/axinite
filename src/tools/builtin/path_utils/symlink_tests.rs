//! Symlink handling in `validate_path`: dangling and live links, links that
//! point inside or outside the sandbox, `..` after a link, and loops.

use std::path::{Path, PathBuf};

use rstest::{fixture, rstest};
use tempfile::{TempDir, tempdir};

use super::validate_path;

/// A sandbox with an `inside/` directory, and a separate directory outside it.
struct Dirs {
    sandbox: TempDir,
    outside: TempDir,
}

#[fixture]
fn dirs() -> Dirs {
    let dirs = Dirs {
        sandbox: tempdir().unwrap(),
        outside: tempdir().unwrap(),
    };
    ambient_fs::create_dir(dirs.sandbox.path().join("inside")).unwrap();
    ambient_fs::create_dir(dirs.outside.path().join("deep")).unwrap();
    dirs
}

/// Create the symlink `sandbox/<name>` pointing at `target`.
fn link(dirs: &Dirs, name: &str, target: impl AsRef<Path>) {
    std::os::unix::fs::symlink(target, dirs.sandbox.path().join(name)).unwrap();
}

/// Where a symlink created by a case points, relative to the fixture.
#[derive(Clone, Copy, Debug)]
enum Target {
    OutsideMissing,
    InsideMissing,
    OutsideDir,
    InsideDir,
    RelativeInsideMissing,
    RelativeOutsideMissing,
}

impl Target {
    fn path(self, dirs: &Dirs) -> PathBuf {
        match self {
            Target::OutsideMissing => dirs.outside.path().join("missing"),
            Target::InsideMissing => dirs.sandbox.path().join("missing"),
            Target::OutsideDir => dirs.outside.path().to_path_buf(),
            Target::InsideDir => dirs.sandbox.path().join("inside"),
            Target::RelativeInsideMissing => PathBuf::from("inside/missing"),
            Target::RelativeOutsideMissing => PathBuf::from("../")
                .join(dirs.outside.path().file_name().unwrap())
                .join("missing"),
        }
    }
}

#[rstest]
#[case::dangling_to_outside(Target::OutsideMissing, "link", false)]
#[case::dangling_to_inside(Target::InsideMissing, "link", true)]
#[case::live_to_outside(Target::OutsideDir, "link", false)]
#[case::live_to_outside_child(Target::OutsideDir, "link/file.txt", false)]
#[case::live_to_inside(Target::InsideDir, "link", true)]
#[case::live_to_inside_child(Target::InsideDir, "link/new.txt", true)]
#[case::relative_dangling_to_inside(Target::RelativeInsideMissing, "link", true)]
#[case::relative_dangling_to_outside(Target::RelativeOutsideMissing, "link", false)]
fn test_validate_path_follows_symlinks(
    dirs: Dirs,
    #[case] target: Target,
    #[case] path: &str,
    #[case] allowed: bool,
) {
    link(&dirs, "link", target.path(&dirs));
    let result = validate_path(path, Some(dirs.sandbox.path()));
    assert_eq!(result.is_ok(), allowed, "{target:?} via {path}");
}

/// `link -> pivot/../victim` with `pivot -> outside/deep`: the `..` applies
/// after `pivot` is resolved, so it lands outside, where `victim` is missing.
#[rstest]
fn test_validate_path_applies_dot_dot_after_resolving_links(dirs: Dirs) {
    link(&dirs, "pivot", dirs.outside.path().join("deep"));
    link(&dirs, "link", "pivot/../victim");
    assert!(validate_path("link", Some(dirs.sandbox.path())).is_err());
}

/// The same shape through a nested link: `sub/l2 -> outside/deep`.
#[rstest]
fn test_validate_path_applies_dot_dot_after_nested_link(dirs: Dirs) {
    ambient_fs::create_dir(dirs.sandbox.path().join("sub")).unwrap();
    std::os::unix::fs::symlink(
        dirs.outside.path().join("deep"),
        dirs.sandbox.path().join("sub/l2"),
    )
    .unwrap();
    link(&dirs, "link", "sub/l2/../missing");
    assert!(validate_path("link", Some(dirs.sandbox.path())).is_err());
}

/// A `..` after an in-sandbox link stays inside.
#[rstest]
fn test_validate_path_allows_dot_dot_after_inside_link(dirs: Dirs) {
    link(&dirs, "pivot", dirs.sandbox.path().join("inside"));
    link(&dirs, "link", "pivot/../missing");
    assert!(validate_path("link", Some(dirs.sandbox.path())).is_ok());
}

/// The input path itself may hold the link: `pivot/../victim` with `pivot`
/// pointing outside resolves outside, not to `sandbox/victim`.
#[rstest]
fn test_validate_path_applies_dot_dot_after_input_link(dirs: Dirs) {
    link(&dirs, "pivot", dirs.outside.path().join("deep"));
    assert!(validate_path("pivot/../victim", Some(dirs.sandbox.path())).is_err());
}

/// An input link to an inside directory resolves, and the returned path no
/// longer holds the link, so a write lands where the check looked.
#[rstest]
fn test_validate_path_returns_a_link_free_path_for_input_links(dirs: Dirs) {
    link(&dirs, "pivot", dirs.sandbox.path().join("inside"));
    let resolved = validate_path("pivot/../new.txt", Some(dirs.sandbox.path())).unwrap();
    let expected = dirs.sandbox.path().canonicalize().unwrap().join("new.txt");
    assert_eq!(resolved, expected);
}

/// An input loop followed by `..` is rejected, not erased.
#[rstest]
fn test_validate_path_rejects_input_loop_before_dot_dot(dirs: Dirs) {
    link(&dirs, "loop", "loop");
    assert!(validate_path("loop/../victim", Some(dirs.sandbox.path())).is_err());
}

/// Loops, including one hidden behind a `..`, are rejected.
#[rstest]
fn test_validate_path_rejects_symlink_loops(dirs: Dirs) {
    link(&dirs, "a", "b");
    link(&dirs, "b", "a");
    assert!(validate_path("a", Some(dirs.sandbox.path())).is_err());

    ambient_fs::create_dir(dirs.sandbox.path().join("sub")).unwrap();
    std::os::unix::fs::symlink("l2", dirs.sandbox.path().join("sub/l2")).unwrap();
    link(&dirs, "hidden", "sub/l2/../missing");
    assert!(validate_path("hidden", Some(dirs.sandbox.path())).is_err());
}
