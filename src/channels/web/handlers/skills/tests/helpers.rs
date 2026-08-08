//! Shared fixtures and request builders for Skills handler tests.

use std::io::Write;
use std::sync::Arc;

use anyhow::Context as _;
use axum::body::to_bytes;
use axum::{Router, routing::post};
use rstest::fixture;

use crate::channels::web::handlers::skills::skills_install_handler;
use crate::channels::web::test_helpers::TestGatewayBuilder;
use crate::skills::registry::SkillRegistry;

pub(crate) struct SkillsApiFixture {
    pub(crate) _installed_dir: tempfile::TempDir,
    pub(crate) _user_dir: tempfile::TempDir,
    pub(crate) state: Arc<crate::channels::web::server::GatewayState>,
    pub(crate) installed_root: std::path::PathBuf,
}

/// Build the Skills API test fixture.
///
/// Arrangement can fail, so the fixture is fallible: consumers take the
/// `Result` and unwrap it in the test body, where a failure is the verdict.
#[fixture]
pub(crate) fn skills_api_fixture() -> anyhow::Result<SkillsApiFixture> {
    let user_dir = tempfile::tempdir().context("user tempdir should be created")?;
    let installed_dir = tempfile::tempdir().context("installed tempdir should be created")?;
    let installed_root = installed_dir.path().to_path_buf();
    let registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(installed_root.clone());
    let registry = Arc::new(std::sync::RwLock::new(registry));
    let state = TestGatewayBuilder::new().skill_registry(registry).build();

    Ok(SkillsApiFixture {
        _installed_dir: installed_dir,
        _user_dir: user_dir,
        state,
        installed_root,
    })
}

pub(crate) fn skills_router(state: Arc<crate::channels::web::server::GatewayState>) -> Router {
    Router::new()
        .route("/api/skills/install", post(skills_install_handler))
        .with_state(state)
}

pub(crate) fn skill_markdown(name: &str) -> String {
    format!("---\nname: {name}\n---\n\n# {name}\n")
}

/// Re-exported bundle archive builder.
///
/// Archive construction can fail, so callers unwrap the `Result` in the test
/// body rather than having the helper panic during arrangement.
pub(crate) use crate::skills::test_support::build_bundle_archive;

pub(crate) enum MultipartPart<'a> {
    File {
        field_name: &'a str,
        file_name: &'a str,
        bytes: &'a [u8],
    },
    FileWithoutFilename {
        field_name: &'a str,
        bytes: &'a [u8],
    },
    Text {
        field_name: &'a str,
        value: &'a str,
    },
}

/// Build a single-file multipart body, returning the content type and payload.
///
/// Body assembly writes into an in-memory buffer, so the write errors are
/// propagated for the test body to unwrap.
pub(crate) fn multipart_file_body(
    field_name: &str,
    file_name: &str,
    bytes: &[u8],
) -> std::io::Result<(String, Vec<u8>)> {
    multipart_body(&[MultipartPart::File {
        field_name,
        file_name,
        bytes,
    }])
}

/// Build a multipart body from the supplied parts, returning the content type
/// and payload.
///
/// Body assembly writes into an in-memory buffer, so the write errors are
/// propagated for the test body to unwrap.
pub(crate) fn multipart_body(parts: &[MultipartPart<'_>]) -> std::io::Result<(String, Vec<u8>)> {
    let boundary = "axinite-skill-boundary";
    let mut body = Vec::new();

    for part in parts {
        match part {
            MultipartPart::File {
                field_name,
                file_name,
                bytes,
            } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )?;
                body.extend_from_slice(bytes);
            }
            MultipartPart::FileWithoutFilename { field_name, bytes } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )?;
                body.extend_from_slice(bytes);
            }
            MultipartPart::Text { field_name, value } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\n\r\n{value}"
                )?;
            }
        }
        write!(body, "\r\n")?;
    }

    write!(body, "\r\n--{boundary}--\r\n")?;
    Ok((format!("multipart/form-data; boundary={boundary}"), body))
}

/// Read a response body as UTF-8 text.
///
/// Reading and decoding can both fail, so the errors are propagated for the
/// test body to unwrap.
pub(crate) async fn response_text(response: axum::response::Response) -> anyhow::Result<String> {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .context("response body should be readable")?;
    String::from_utf8(bytes.to_vec()).context("response body should be UTF-8")
}
