//! Structural JSON limits shared by both peers.
//!
//! The scanner runs before typed deserialization, so an attacker cannot use
//! deeply nested input or an unrepresentable number to exhaust the peer before
//! the message contract is checked.

/// Maximum nesting accepted in any frame.
pub const MAX_JSON_DEPTH: usize = 64;

/// Maximum aggregate size of schema-derived guest stubs.
///
/// The 2.2.1 helper exposes tools as named callables without generated type
/// stubs, so the limit is reserved and enforced by the parent when it later
/// emits stubs.
pub const MAX_STUB_BYTES: usize = 256 * 1024;

/// A structural or numeric limit failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JsonLimitError {
    /// The value nests deeper than [`MAX_JSON_DEPTH`].
    #[error("JSON nesting exceeds the {MAX_JSON_DEPTH}-level ceiling")]
    DepthExceeded,
    /// An integer did not fit in a signed 64-bit value.
    #[error("JSON integer does not fit a signed 64-bit value: {0}")]
    IntegerOutOfRange(String),
    /// A number was not finite binary64.
    #[error("JSON number is not a finite binary64 value: {0}")]
    NonFiniteNumber(String),
}

/// Validates nesting depth and number representation in JSON `text`.
///
/// Malformed JSON syntax is left to `serde_json`; this scanner only enforces
/// the limits that must apply before typed deserialization.
pub fn validate_json_text(text: &str) -> Result<(), JsonLimitError> {
    let bytes = text.as_bytes();
    let mut depth = 0_usize;
    let mut index = 0_usize;

    while index < bytes.len() {
        index = match bytes[index] {
            b'"' => skip_string(bytes, index),
            b'{' | b'[' => {
                depth = open_container(depth)?;
                index + 1
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                index + 1
            }
            b'-' | b'0'..=b'9' => skip_number(text, bytes, index)?,
            _ => index + 1,
        };
    }
    Ok(())
}

/// Advances past the string literal that opens at `start`.
///
/// An unterminated literal ends at the end of input; malformed syntax is
/// `serde_json`'s concern, and the scanner only has to avoid misreading a
/// structural byte inside it.
fn skip_string(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    let mut escaped = false;
    while index < bytes.len() {
        match bytes[index] {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' => return index + 1,
            _ => {}
        }
        index += 1;
    }
    index
}

/// Advances past the number token that starts at `start`, validating it.
fn skip_number(text: &str, bytes: &[u8], start: usize) -> Result<usize, JsonLimitError> {
    let mut index = start + 1;
    while index < bytes.len()
        && matches!(bytes[index], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
    {
        index += 1;
    }
    validate_number(&text[start..index])?;
    Ok(index)
}

/// Counts one opening container, rejecting the level above the ceiling.
fn open_container(depth: usize) -> Result<usize, JsonLimitError> {
    let depth = depth.saturating_add(1);
    if depth > MAX_JSON_DEPTH {
        Err(JsonLimitError::DepthExceeded)
    } else {
        Ok(depth)
    }
}

fn validate_number(token: &str) -> Result<(), JsonLimitError> {
    if token.contains(['.', 'e', 'E']) {
        let value = token
            .parse::<f64>()
            .map_err(|_| JsonLimitError::NonFiniteNumber(token.to_owned()))?;
        if value.is_finite() {
            Ok(())
        } else {
            Err(JsonLimitError::NonFiniteNumber(token.to_owned()))
        }
    } else {
        token
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| JsonLimitError::IntegerOutOfRange(token.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the structural scanner.

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn accepts_ordinary_json() {
        validate_json_text(r#"{"a":[1,-2,3.5e2,true,null,"x\\"]}"#).expect("valid");
    }

    #[test]
    fn numbers_inside_strings_are_not_scanned() {
        validate_json_text(r#"{"text":"999999999999999999999999"}"#).expect("string");
    }

    #[test]
    fn rejects_integer_outside_i64() {
        let error = validate_json_text("9223372036854775808").expect_err("range");
        assert_eq!(
            error,
            JsonLimitError::IntegerOutOfRange("9223372036854775808".to_owned())
        );
    }

    #[test]
    fn rejects_non_finite_float() {
        let error = validate_json_text("1e400").expect_err("non-finite");
        assert_eq!(error, JsonLimitError::NonFiniteNumber("1e400".to_owned()));
    }

    #[test]
    fn rejects_excessive_depth() {
        let text = "[".repeat(MAX_JSON_DEPTH + 1);
        let error = validate_json_text(&text).expect_err("depth");
        assert_eq!(error, JsonLimitError::DepthExceeded);
    }

    #[test]
    fn accepts_depth_at_the_limit() {
        let text = "[".repeat(MAX_JSON_DEPTH);
        validate_json_text(&text).expect("depth");
    }
}
