//! Unit tests for the container worker runtime and its tool-advertising paths.

use rstest::rstest;

use super::*;

mod pre_loop;
mod remote_tools;
pub mod test_support;

#[rstest]
#[tokio::test]
async fn worker_runtime_build_tools_preserves_container_local_tools() {
    let mut names = WorkerRuntime::build_tools().list().await;
    names.sort();

    assert_eq!(
        names,
        vec![
            "apply_patch",
            "list_dir",
            "read_file",
            "shell",
            "write_file"
        ]
    );
}
