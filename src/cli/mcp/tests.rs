//! Unit tests for MCP command parsing and header validation.

use super::command::{parse_env_var, parse_header};
use super::*;

#[test]
fn test_mcp_command_parsing() {
    // Just verify the command structure is valid
    use clap::CommandFactory;

    // Create a dummy parent command to test subcommand parsing
    #[derive(clap::Parser)]
    struct TestCli {
        #[command(subcommand)]
        cmd: McpCommand,
    }

    TestCli::command().debug_assert();
}

#[test]
fn test_parse_header_valid() {
    let result = parse_header("Authorization: Bearer token123").unwrap();
    assert_eq!(result.0, "Authorization");
    assert_eq!(result.1, "Bearer token123");
}

#[test]
fn test_parse_header_no_spaces() {
    let result = parse_header("X-Api-Key:abc123").unwrap();
    assert_eq!(result.0, "X-Api-Key");
    assert_eq!(result.1, "abc123");
}

#[test]
fn test_parse_header_invalid() {
    let result = parse_header("no-colon-here");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid header format"));
}

#[test]
fn test_parse_env_var_valid() {
    let result = parse_env_var("NODE_ENV=production").unwrap();
    assert_eq!(result.0, "NODE_ENV");
    assert_eq!(result.1, "production");
}

#[test]
fn test_parse_env_var_with_equals_in_value() {
    let result = parse_env_var("KEY=value=with=equals").unwrap();
    assert_eq!(result.0, "KEY");
    assert_eq!(result.1, "value=with=equals");
}

#[test]
fn test_parse_env_var_invalid() {
    let result = parse_env_var("no-equals-here");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid env var format"));
}
