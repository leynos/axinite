//! URL sanitization helpers for safe display of URLs in startup output.

use url::{Url, form_urlencoded};

/// Removes credentials and redacts sensitive query parameters from a URL
/// string for safe display.
pub(crate) fn sanitize_display_url(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return sanitize_relative_display_url(url);
    };

    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);

    let had_query = parsed.query().is_some();
    let sanitized_pairs = sanitize_query_pairs(parsed.query_pairs());

    parsed.set_query(None);
    if had_query && sanitized_pairs.is_empty() {
        parsed.set_query(Some(""));
    } else if had_query && !sanitized_pairs.is_empty() {
        let mut query = parsed.query_pairs_mut();
        query.extend_pairs(
            sanitized_pairs
                .iter()
                .map(|(key, value)| (&**key, &**value)),
        );
    }

    parsed.to_string()
}

/// Sanitizes a relative or protocol-relative URL string for display.
fn sanitize_relative_display_url(url: &str) -> String {
    let (prefix, fragment) = match url.split_once('#') {
        Some((prefix, fragment)) => (prefix, Some(fragment)),
        None => (url, None),
    };
    let (prefix, query) = match prefix.split_once('?') {
        Some((prefix, query)) => (prefix, Some(query)),
        None => (prefix, None),
    };
    let sanitized_prefix = strip_authority_credentials(prefix);
    let Some(query) = query else {
        return match fragment {
            Some(fragment) => format!("{sanitized_prefix}#{fragment}"),
            None => sanitized_prefix,
        };
    };
    let sanitized_query = sanitize_query_string(query);
    match fragment {
        Some(fragment) => format!("{sanitized_prefix}?{sanitized_query}#{fragment}"),
        None => format!("{sanitized_prefix}?{sanitized_query}"),
    }
}

/// Strips `user:password@` from the authority component of a URL string.
fn strip_authority_credentials(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        return format!("{scheme}://{}", strip_credentials_from_authority(rest));
    }
    if let Some(rest) = url.strip_prefix("//") {
        return format!("//{}", strip_credentials_from_authority(rest));
    }
    url.to_string()
}

/// Removes `user:pass@` from a bare authority string (no scheme prefix).
fn strip_credentials_from_authority(rest: &str) -> String {
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, suffix) = rest.split_at(authority_end);
    let redacted_authority = authority
        .rsplit_once('@')
        .map_or_else(|| authority.to_string(), |(_, host)| host.to_string());
    format!("{redacted_authority}{suffix}")
}

/// Redacts values of sensitive query keys in a raw query string.
fn sanitize_query_string(query: &str) -> String {
    let sanitized_pairs = sanitize_query_pairs(form_urlencoded::parse(query.as_bytes()));
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(
        sanitized_pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    serializer.finish()
}

/// Redacts values of sensitive keys from an iterator of query key-value
/// pairs.
fn sanitize_query_pairs<'a, I>(pairs: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (std::borrow::Cow<'a, str>, std::borrow::Cow<'a, str>)>,
{
    pairs
        .into_iter()
        .map(|(key, value)| {
            if should_redact_query_key(&key) {
                (key.into_owned(), "[REDACTED]".to_string())
            } else {
                (key.into_owned(), value.into_owned())
            }
        })
        .collect()
}

/// Returns `true` when the given query key name is considered sensitive
/// (e.g. `token`, `api_key`, `authorization`).
fn should_redact_query_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "token"
            | "access_token"
            | "authorization"
            | "api_key"
            | "apikey"
            | "secret"
            | "password"
            | "pass"
            | "key"
            | "client_secret"
            | "auth"
    )
}
