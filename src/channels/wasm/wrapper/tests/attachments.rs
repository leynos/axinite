use rstest::rstest;

use super::super::attachments::mime_from_extension;

#[rstest]
#[case("screenshot.png", "image/png")]
#[case("photo.JPG", "image/jpeg")]
#[case("photo.jpeg", "image/jpeg")]
#[case("animation.gif", "image/gif")]
#[case("doc.pdf", "application/pdf")]
#[case("video.mp4", "video/mp4")]
#[case("data.csv", "text/csv")]
#[case("unknown.qqqzzz", "application/octet-stream")]
#[case("noext", "application/octet-stream")]
#[case("/home/user/.ironclaw/screenshot.png", "image/png")]
fn test_mime_from_extension(#[case] filename: &str, #[case] expected: &str) {
    assert_eq!(mime_from_extension(filename), expected);
}
