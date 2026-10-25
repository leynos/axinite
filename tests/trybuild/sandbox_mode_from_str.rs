//! Compile-contract fixture for the public sandbox mode parser.

use std::str::FromStr;

use axinite::db::{SandboxMode, SandboxModeParseError};

fn assert_public_error_type<T: FromStr<Err = SandboxModeParseError>>() {}

fn main() {
    assert_public_error_type::<SandboxMode>();
    let _: Result<SandboxMode, SandboxModeParseError> = "worker".parse::<SandboxMode>();
}
