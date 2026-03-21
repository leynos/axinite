//! Shared skill-fetch helpers with SSRF and archive-safety checks.

mod http;
#[cfg(test)]
mod tests;
mod url_policy;
mod zip_extract;

pub(crate) use http::fetch_skill_content;

#[cfg(test)]
use url_policy::{is_private_ip, validate_fetch_url, validate_resolved_addrs};
#[cfg(test)]
use zip_extract::extract_skill_from_zip;
