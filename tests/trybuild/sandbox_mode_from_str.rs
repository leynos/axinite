//! Compile-contract fixture for the public sandbox mode parser.

use std::str::FromStr;

use axinite::db::{SandboxMode, SandboxModeParseError};

fn assert_public_parser_types<T>()
where
    T: FromStr<Err = SandboxModeParseError>
        + TryFrom<&'static str, Error = SandboxModeParseError>,
{
}

fn main() {
    assert_public_parser_types::<SandboxMode>();
    let _: Result<SandboxMode, SandboxModeParseError> = "worker".parse::<SandboxMode>();
    let _: Result<SandboxMode, SandboxModeParseError> = SandboxMode::try_from("worker");
}
