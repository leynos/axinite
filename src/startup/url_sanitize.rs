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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use proptest::{prelude::*, strategy::Strategy};

    use super::*;

    const SENSITIVE_KEYS: &[&str] = &[
        "token",
        "access_token",
        "authorization",
        "api_key",
        "apikey",
        "secret",
        "password",
        "pass",
        "key",
        "client_secret",
        "auth",
    ];

    fn casing_variant(base: &'static str) -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<bool>(), base.len()).prop_map(move |uppercase| {
            base.chars()
                .zip(uppercase)
                .map(|(ch, should_uppercase)| {
                    if ch.is_ascii_alphabetic() && should_uppercase {
                        ch.to_ascii_uppercase()
                    } else {
                        ch.to_ascii_lowercase()
                    }
                })
                .collect()
        })
    }

    fn sensitive_key_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            casing_variant("token"),
            casing_variant("access_token"),
            casing_variant("authorization"),
            casing_variant("api_key"),
            casing_variant("apikey"),
            casing_variant("secret"),
            casing_variant("password"),
            casing_variant("pass"),
            casing_variant("key"),
            casing_variant("client_secret"),
            casing_variant("auth"),
        ]
    }

    fn safe_value_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[A-Za-z0-9._~-]{1,16}")
            .expect("value regex should compile")
    }

    fn optional_safe_value_strategy() -> impl Strategy<Value = Option<String>> {
        prop_oneof![Just(None), safe_value_strategy().prop_map(Some),]
    }

    fn host_strategy() -> impl Strategy<Value = String> {
        prop::string::string_regex("[a-z]{1,10}(?:-[a-z]{1,10})?\\.example\\.test")
            .expect("host regex should compile")
    }

    fn noise_pairs_strategy() -> impl Strategy<Value = Vec<(Cow<'static, str>, Cow<'static, str>)>> {
        proptest::collection::vec(
            (
                prop::sample::select(vec!["mode", "page", "lang", "view"]),
                safe_value_strategy(),
            ),
            0..3,
        )
        .prop_map(|pairs| {
            pairs
                .into_iter()
                .map(|(key, value)| (Cow::Borrowed(key), Cow::Owned(value)))
                .collect()
        })
    }

    proptest! {
        #[test]
        fn sanitize_query_pairs_never_leaks_a_sensitive_value(
            sensitive_key in sensitive_key_strategy(),
            sensitive_value in safe_value_strategy(),
            noise_pairs in noise_pairs_strategy(),
        ) {
            let mut pairs = vec![(Cow::Owned(sensitive_key.clone()), Cow::Owned(sensitive_value))];
            pairs.extend(noise_pairs);

            let sanitized = sanitize_query_pairs(pairs.into_iter());

            for (key, value) in sanitized {
                if SENSITIVE_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                    prop_assert_eq!(value, "[REDACTED]");
                }
            }
        }

        #[test]
        fn should_redact_query_key_is_case_insensitive(
            sensitive_key in sensitive_key_strategy(),
        ) {
            prop_assert!(should_redact_query_key(&sensitive_key));
        }

        #[test]
        fn sanitize_query_string_never_leaks_sensitive_values(
            sensitive_key in sensitive_key_strategy(),
            sensitive_value in safe_value_strategy(),
        ) {
            let mut serializer = form_urlencoded::Serializer::new(String::new());
            serializer.append_pair(&sensitive_key, &sensitive_value);
            let query = serializer.finish();

            let sanitized = sanitize_query_string(&query);
            let sanitized_pairs: Vec<(String, String)> =
                form_urlencoded::parse(sanitized.as_bytes()).into_owned().collect();
            let all_sensitive_values_redacted = sanitized_pairs.into_iter().all(|(key, value)| {
                !key.eq_ignore_ascii_case(&sensitive_key) || value == "[REDACTED]"
            });

            prop_assume!(should_redact_query_key(&sensitive_key));
            prop_assert!(all_sensitive_values_redacted);
        }

        #[test]
        fn sanitize_display_url_absolute_never_leaks_credentials_or_sensitive_query_values(
            host in host_strategy(),
            user in optional_safe_value_strategy(),
            password in optional_safe_value_strategy(),
            sensitive_key in sensitive_key_strategy(),
            sensitive_value in safe_value_strategy(),
        ) {
            let auth = match (&user, &password) {
                (Some(user), Some(password)) => format!("{user}:{password}@"),
                (Some(user), None) => format!("{user}@"),
                (None, _) => String::new(),
            };
            let mut serializer = form_urlencoded::Serializer::new(String::new());
            serializer.append_pair(&sensitive_key, &sensitive_value);
            let url = format!("https://{auth}{host}/?{}", serializer.finish());

            let sanitized = sanitize_display_url(&url);
            let parsed = Url::parse(&sanitized).expect("sanitized URL should remain absolute");
            let all_sensitive_values_redacted = parsed.query_pairs().all(|(key, value)| {
                !key.eq_ignore_ascii_case(&sensitive_key) || value == "[REDACTED]"
            });

            if let (Some(user), Some(password)) = (&user, &password) {
                let credential_fragment = format!("{user}:{password}@");
                prop_assert!(!sanitized.contains(&credential_fragment));
            }
            prop_assert_eq!(parsed.password(), None);
            prop_assert!(all_sensitive_values_redacted);
        }

        #[test]
        fn sanitize_relative_display_url_never_leaks_credentials_or_sensitive_query_values(
            host in host_strategy(),
            user in optional_safe_value_strategy(),
            password in optional_safe_value_strategy(),
            sensitive_key in sensitive_key_strategy(),
            sensitive_value in safe_value_strategy(),
        ) {
            let auth = match (&user, &password) {
                (Some(user), Some(password)) => format!("{user}:{password}@"),
                (Some(user), None) => format!("{user}@"),
                (None, _) => String::new(),
            };
            let mut serializer = form_urlencoded::Serializer::new(String::new());
            serializer.append_pair(&sensitive_key, &sensitive_value);
            let url = format!("//{auth}{host}/?{}", serializer.finish());

            let sanitized = sanitize_relative_display_url(&url);
            let query = sanitized
                .split_once('?')
                .map(|(_, query)| query)
                .unwrap_or_default();
            let sanitized_pairs: Vec<(String, String)> =
                form_urlencoded::parse(query.as_bytes()).into_owned().collect();
            let all_sensitive_values_redacted = sanitized_pairs.into_iter().all(|(key, value)| {
                !key.eq_ignore_ascii_case(&sensitive_key) || value == "[REDACTED]"
            });

            if let (Some(user), Some(password)) = (&user, &password) {
                let credential_fragment = format!("{user}:{password}@");
                prop_assert!(!sanitized.contains(&credential_fragment));
            }
            prop_assert!(all_sensitive_values_redacted);
        }
    }
}
