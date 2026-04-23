//! Shared skill-fetch helpers with SSRF and archive-safety checks.

mod http;
#[cfg(test)]
mod tests;
mod url_policy;

pub(crate) use http::fetch_skill_bytes;

#[cfg(test)]
use url_policy::{is_private_ip, validate_fetch_url, validate_resolved_addrs};
