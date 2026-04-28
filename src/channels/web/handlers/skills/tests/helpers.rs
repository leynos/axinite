//! Shared fixtures and request builders for Skills handler tests.

use std::io::Write;
use std::sync::Arc;

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

#[fixture]
pub(crate) fn skills_api_fixture() -> SkillsApiFixture {
    let user_dir = tempfile::tempdir().expect("user tempdir should be created");
    let installed_dir = tempfile::tempdir().expect("installed tempdir should be created");
    let installed_root = installed_dir.path().to_path_buf();
    let registry = SkillRegistry::new(user_dir.path().to_path_buf())
        .with_installed_dir(installed_root.clone());
    let registry = Arc::new(std::sync::RwLock::new(registry));
    let state = TestGatewayBuilder::new().skill_registry(registry).build();

    SkillsApiFixture {
        _installed_dir: installed_dir,
        _user_dir: user_dir,
        state,
        installed_root,
    }
}

pub(crate) fn skills_router(state: Arc<crate::channels::web::server::GatewayState>) -> Router {
    Router::new()
        .route("/api/skills/install", post(skills_install_handler))
        .with_state(state)
}

pub(crate) fn skill_markdown(name: &str) -> String {
    format!("---\nname: {name}\n---\n\n# {name}\n")
}

pub(crate) fn build_bundle_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for (name, contents) in entries {
        writer
            .start_file(*name, options)
            .expect("test archive should start file");
        writer
            .write_all(contents)
            .expect("test archive should write file contents");
    }

    writer
        .finish()
        .expect("test archive should finish")
        .into_inner()
}

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

pub(crate) fn multipart_file_body(
    field_name: &str,
    file_name: &str,
    bytes: &[u8],
) -> (String, Vec<u8>) {
    multipart_body(&[MultipartPart::File {
        field_name,
        file_name,
        bytes,
    }])
}

pub(crate) fn multipart_body(parts: &[MultipartPart<'_>]) -> (String, Vec<u8>) {
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
                )
                .expect("multipart file header should write");
                body.extend_from_slice(bytes);
            }
            MultipartPart::FileWithoutFilename { field_name, bytes } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
                )
                .expect("multipart file header should write");
                body.extend_from_slice(bytes);
            }
            MultipartPart::Text { field_name, value } => {
                write!(
                    body,
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{field_name}\"\r\n\r\n{value}"
                )
                .expect("multipart text field should write");
            }
        }
        write!(body, "\r\n").expect("multipart separator should write");
    }

    write!(body, "\r\n--{boundary}--\r\n").expect("multipart footer should write");
    (format!("multipart/form-data; boundary={boundary}"), body)
}

pub(crate) async fn response_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body should be readable");
    String::from_utf8(bytes.to_vec()).expect("response body should be UTF-8")
}
