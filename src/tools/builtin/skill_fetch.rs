//! Shared skill-fetch helpers with SSRF and archive-safety checks.

use std::io::{Read, Take};
use std::net::{IpAddr, SocketAddr};

use flate2::read::DeflateDecoder;
use futures::StreamExt;

use crate::tools::tool::ToolError;

const USER_AGENT: &str = concat!("ironclaw/", env!("CARGO_PKG_VERSION"));
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_DECOMPRESSED: usize = 1024 * 1024;

#[cfg(test)]
mod tests;

/// Return `true` when the lowercased, normalised hostname is known to resolve
/// to an internal or metadata endpoint that must not be fetched.
fn is_blocked_hostname(host_lower: &str) -> bool {
    host_lower == "localhost"
        || host_lower == "metadata.google.internal"
        || host_lower.ends_with(".internal")
        || host_lower.ends_with(".local")
}

/// Validate that a URL is safe to fetch.
fn validate_fetch_url(url_str: &str) -> Result<reqwest::Url, ToolError> {
    let parsed = reqwest::Url::parse(url_str)
        .map_err(|e| ToolError::ExecutionFailed(format!("Invalid URL '{}': {}", url_str, e)))?;

    if parsed.scheme() != "https" {
        return Err(ToolError::ExecutionFailed(format!(
            "Only HTTPS URLs are allowed for skill fetching, got scheme '{}'",
            parsed.scheme()
        )));
    }

    let host = parsed
        .host()
        .ok_or_else(|| ToolError::ExecutionFailed("URL has no host".to_string()))?;

    if let Some(ip) = host_ip_addr(&host) {
        validate_fetch_ip(&ip, &host.to_string())?;
    }

    let host_lower = normalize_domain(host.to_string().as_str()).to_lowercase();
    if is_blocked_hostname(&host_lower) {
        return Err(ToolError::ExecutionFailed(format!(
            "URL points to an internal hostname: {}",
            host
        )));
    }

    Ok(parsed)
}

fn host_ip_addr(host: &url::Host<&str>) -> Option<IpAddr> {
    match host {
        url::Host::Ipv4(v4) => Some(IpAddr::V4(*v4)),
        url::Host::Ipv6(v6) => Some(normalize_ip(IpAddr::V6(*v6))),
        url::Host::Domain(_) => None,
    }
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

/// Return `true` when the IP address is loopback, unspecified, private,
/// or link-local, and therefore must not be fetched.
fn is_non_routable_ip(ip: &IpAddr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || is_private_ip(ip) || is_link_local_ip(ip)
}

fn validate_fetch_ip(ip: &IpAddr, display_host: &str) -> Result<(), ToolError> {
    if is_non_routable_ip(ip) {
        return Err(ToolError::ExecutionFailed(format!(
            "URL points to a private/loopback/link-local address: {}",
            display_host
        )));
    }

    Ok(())
}

fn normalize_domain(host: &str) -> &str {
    host.trim_end_matches('.')
}

fn validate_resolved_addrs(host: &str, addrs: &[SocketAddr]) -> Result<(), ToolError> {
    if addrs.is_empty() {
        return Err(ToolError::ExecutionFailed(format!(
            "DNS resolution returned no addresses for {}",
            host
        )));
    }

    for addr in addrs {
        let ip = normalize_ip(addr.ip());
        validate_fetch_ip(&ip, host)?;
    }

    Ok(())
}

fn build_fetch_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
}

async fn build_safe_fetch_client(parsed: &reqwest::Url) -> Result<reqwest::Client, ToolError> {
    let host = parsed
        .host()
        .ok_or_else(|| ToolError::ExecutionFailed("URL has no host".to_string()))?;

    match host {
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => build_fetch_client_builder()
            .build()
            .map_err(|e| ToolError::ExecutionFailed(format!("HTTP client error: {}", e))),
        url::Host::Domain(domain) => {
            let lookup_host = normalize_domain(domain);
            let port = parsed
                .port_or_known_default()
                .ok_or_else(|| ToolError::ExecutionFailed("URL has no valid port".to_string()))?;

            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((lookup_host, port))
                .await
                .map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "DNS resolution failed for {}: {}",
                        lookup_host, e
                    ))
                })?
                .collect();

            validate_resolved_addrs(domain, &addrs)?;

            build_fetch_client_builder()
                .resolve_to_addrs(domain, &addrs)
                .build()
                .map_err(|e| ToolError::ExecutionFailed(format!("HTTP client error: {}", e)))
        }
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            (segments[0] & 0xfe00) == 0xfc00
        }
    }
}

fn is_link_local_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            (segments[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Fetch SKILL.md content from a URL with SSRF protection.
pub(crate) async fn fetch_skill_content(url: &str) -> Result<String, ToolError> {
    let parsed = validate_fetch_url(url)?;
    let client = build_safe_fetch_client(&parsed).await?;

    let response = client.get(parsed.clone()).send().await.map_err(|e| {
        ToolError::ExecutionFailed(format!("Failed to fetch skill from {}: {}", url, e))
    })?;

    if !response.status().is_success() {
        return Err(ToolError::ExecutionFailed(format!(
            "Skill fetch returned HTTP {}: {}",
            response.status(),
            url
        )));
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read response body: {}", e))
        })?;
        let next_len = bytes.len().saturating_add(chunk.len());
        if next_len > MAX_DOWNLOAD_BYTES {
            return Err(ToolError::ExecutionFailed(format!(
                "Response too large: {} bytes (max {} bytes)",
                next_len, MAX_DOWNLOAD_BYTES
            )));
        }
        bytes.extend_from_slice(&chunk);
    }

    if bytes.starts_with(b"PK\x03\x04") {
        extract_skill_from_zip(&bytes)
    } else {
        String::from_utf8(bytes)
            .map_err(|e| ToolError::ExecutionFailed(format!("Response is not valid UTF-8: {}", e)))
    }
}

/// Parsed fields from a ZIP local-file header (signature `PK\x03\x04`).
struct ZipLocalHeader {
    compression: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    name_start: usize,
    name_end: usize,
    extra_len: usize,
}

/// Parse a ZIP local-file header at `offset` into a [`ZipLocalHeader`].
///
/// Callers must enforce the precondition `offset + 30 <= data.len()` before
/// calling this function. If the four-byte signature does not match
/// `0x50 0x4B 0x03 0x04`, this returns `None`. Violating the length
/// precondition causes out-of-bounds panics rather than a safe error, so
/// callers must validate buffer bounds first. Callers must also validate that
/// `data.len() >= name_end + extra_len` before treating the parsed filename and
/// extra-field ranges as safe to read.
fn parse_zip_local_header(data: &[u8], offset: usize) -> Option<ZipLocalHeader> {
    if data[offset..offset + 4] != [0x50, 0x4B, 0x03, 0x04] {
        return None;
    }
    let compression = u16::from_le_bytes([data[offset + 8], data[offset + 9]]);
    let compressed_size = u32::from_le_bytes([
        data[offset + 18],
        data[offset + 19],
        data[offset + 20],
        data[offset + 21],
    ]) as usize;
    let uncompressed_size = u32::from_le_bytes([
        data[offset + 22],
        data[offset + 23],
        data[offset + 24],
        data[offset + 25],
    ]) as usize;
    let name_len = u16::from_le_bytes([data[offset + 26], data[offset + 27]]) as usize;
    let extra_len = u16::from_le_bytes([data[offset + 28], data[offset + 29]]) as usize;
    let name_start = offset + 30;
    let name_end = name_start + name_len;
    Some(ZipLocalHeader {
        compression,
        compressed_size,
        uncompressed_size,
        name_start,
        name_end,
        extra_len,
    })
}

/// Decompress `raw` bytes using ZIP `compression` method 0 (stored) or
/// 8 (deflate). Returns an error for any other method.
fn decompress_zip_entry(
    raw: &[u8],
    compression: u16,
    uncompressed_size: usize,
) -> Result<Vec<u8>, ToolError> {
    if raw.len() > MAX_DECOMPRESSED {
        return Err(ToolError::ExecutionFailed(
            "ZIP entry too large to decompress safely".to_string(),
        ));
    }

    match compression {
        0 => {
            if raw.len() != uncompressed_size {
                return Err(ToolError::ExecutionFailed(
                    "ZIP archive truncated".to_string(),
                ));
            }
            Ok(raw.to_vec())
        }
        8 => {
            let mut decoder: Take<DeflateDecoder<&[u8]>> =
                DeflateDecoder::new(raw).take(MAX_DECOMPRESSED as u64);
            let mut buf = Vec::with_capacity(uncompressed_size.min(MAX_DECOMPRESSED));
            decoder.read_to_end(&mut buf).map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to decompress SKILL.md: {}", e))
            })?;
            if buf.len() > MAX_DECOMPRESSED {
                return Err(ToolError::ExecutionFailed(
                    "ZIP entry too large to decompress safely".to_string(),
                ));
            }
            if buf.len() == MAX_DECOMPRESSED && uncompressed_size > MAX_DECOMPRESSED {
                return Err(ToolError::ExecutionFailed(
                    "ZIP entry too large to decompress safely".to_string(),
                ));
            }
            if buf.len() != uncompressed_size {
                return Err(ToolError::ExecutionFailed(
                    "ZIP archive truncated".to_string(),
                ));
            }
            Ok(buf)
        }
        other => Err(ToolError::ExecutionFailed(format!(
            "Unsupported ZIP compression method: {}",
            other
        ))),
    }
}

/// Parameters for extracting a single `SKILL.md` archive entry.
struct SkillEntryParams<'a> {
    data: &'a [u8],
    data_start: usize,
    data_end: usize,
    compression: u16,
    uncompressed_size: usize,
}

/// Validate bounds and size, decompress, and decode `SKILL.md` bytes to UTF-8.
fn extract_skill_entry(args: SkillEntryParams<'_>) -> Result<String, ToolError> {
    if args.data_end > args.data.len() {
        return Err(ToolError::ExecutionFailed(
            "ZIP archive truncated".to_string(),
        ));
    }
    if args.uncompressed_size > MAX_DECOMPRESSED {
        return Err(ToolError::ExecutionFailed(
            "ZIP entry too large to decompress safely".to_string(),
        ));
    }
    let decompressed = decompress_zip_entry(
        &args.data[args.data_start..args.data_end],
        args.compression,
        args.uncompressed_size,
    )?;
    String::from_utf8(decompressed).map_err(|e| {
        ToolError::ExecutionFailed(format!("SKILL.md in archive is not valid UTF-8: {}", e))
    })
}

/// Extract the root `SKILL.md` payload from a complete ZIP archive.
///
/// This function expects a complete ZIP archive as untrusted `&[u8]` input and
/// performs manual local-header parsing rather than relying on a high-level ZIP
/// library. It scans entries in local-header order, requires a root filename of
/// exactly `SKILL.md`, and validates header bounds, `extra_len`,
/// `compressed_size`, and `uncompressed_size` before passing matching entries
/// to [`extract_skill_entry`] for decompression and UTF-8 validation.
///
/// The parser enforces the configured size constraints for compressed input,
/// decompressed entry data, filename-derived offsets, and checked
/// offset-and-length arithmetic. Callers must treat the provided bytes as
/// untrusted input, and this function will reject malformed, truncated, or
/// oversized archives before attempting to return the skill payload.
///
/// Returns `Err` when offset arithmetic overflows (`ZIP header offset
/// overflow`, `ZIP header size overflow`), when entry data points out of
/// bounds, when `SKILL.md` is missing, or when [`extract_skill_entry`]
/// reports truncation, unsupported compression, invalid UTF-8 payload bytes, or
/// other [`ToolError::ExecutionFailed`] validation failures. Entry names with
/// invalid UTF-8 are treated as non-matching, which eventually yields the
/// missing-`SKILL.md` error.
fn extract_skill_from_zip(data: &[u8]) -> Result<String, ToolError> {
    let mut offset = 0usize;

    while offset + 30 <= data.len() {
        let header = match parse_zip_local_header(data, offset) {
            Some(h) => h,
            None => break,
        };

        if header.name_end > data.len() {
            break;
        }
        let file_name =
            std::str::from_utf8(&data[header.name_start..header.name_end]).unwrap_or("");

        let data_start = header
            .name_end
            .checked_add(header.extra_len)
            .ok_or_else(|| ToolError::ExecutionFailed("ZIP header offset overflow".to_string()))?;
        let data_end = data_start
            .checked_add(header.compressed_size)
            .ok_or_else(|| ToolError::ExecutionFailed("ZIP header size overflow".to_string()))?;

        if file_name == "SKILL.md" {
            return extract_skill_entry(SkillEntryParams {
                data,
                data_start,
                data_end,
                compression: header.compression,
                uncompressed_size: header.uncompressed_size,
            });
        }

        offset = data_end;
    }

    Err(ToolError::ExecutionFailed(
        "ZIP archive does not contain SKILL.md".to_string(),
    ))
}
