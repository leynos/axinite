//! Raw source templates for generated tools: WASM crates, CLI binaries,
//! and Python/Bash scripts, with {{placeholder}} substitution points.

// =============================================================================
// WASM Templates
// =============================================================================

pub(super) const WASM_CARGO_TOML: &str = r##"[package]
name = "{{name}}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.release]
opt-level = "s"
lto = true
"##;

pub(super) const WASM_HTTP_LIB_RS: &str = r##"//! {{description}}
//!
//! This WASM tool makes HTTP requests to external APIs.

use serde::{Deserialize, Serialize};

// Host function imports
#[link(wasm_import_module = "env")]
extern "C" {
    fn host_log(level: i32, ptr: *const u8, len: usize);
    fn host_http_request(
        method_ptr: *const u8, method_len: usize,
        url_ptr: *const u8, url_len: usize,
        headers_ptr: *const u8, headers_len: usize,
        body_ptr: *const u8, body_len: usize,
        response_ptr: *mut u8, response_max_len: usize,
    ) -> i32;
}

fn log_info(msg: &str) {
    unsafe { host_log(1, msg.as_ptr(), msg.len()); }
}

fn http_get(url: &str) -> Result<String, String> {
    let method = "GET";
    let mut response_buf = vec![0u8; 65536];
    let result = unsafe {
        host_http_request(
            method.as_ptr(), method.len(),
            url.as_ptr(), url.len(),
            std::ptr::null(), 0,
            std::ptr::null(), 0,
            response_buf.as_mut_ptr(), response_buf.len(),
        )
    };
    if result < 0 { return Err(format!("HTTP error: {}", result)); }
    response_buf.truncate(result as usize);
    String::from_utf8(response_buf).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct Input {
    {{input_fields}}
}

#[derive(Serialize)]
struct Output {
    {{output_fields}}
}

#[no_mangle]
pub extern "C" fn run(input_ptr: *const u8, input_len: usize) -> u64 {
    let result = run_inner(input_ptr, input_len);
    let json = match result {
        Ok(output) => serde_json::to_string(&output).unwrap_or_else(|e| {
            format!("{{\"error\":\"serialize: {}\"}}", e)
        }),
        Err(e) => format!("{{\"error\":\"{}\"}}", e.replace('"', "'")),
    };
    let bytes = json.into_bytes();
    let ptr = bytes.as_ptr() as u64;
    let len = bytes.len() as u64;
    std::mem::forget(bytes);
    (len << 32) | ptr
}

fn run_inner(input_ptr: *const u8, input_len: usize) -> Result<Output, String> {
    let input_bytes = unsafe { std::slice::from_raw_parts(input_ptr, input_len) };
    let input: Input = serde_json::from_slice(input_bytes)
        .map_err(|e| format!("Invalid input: {}", e))?;

    log_info("Processing request...");

    {{implementation}}

    Ok(Output {
        {{output_construction}}
    })
}
"##;

pub(super) const WASM_TRANSFORM_LIB_RS: &str = r##"//! {{description}}
//!
//! This WASM tool transforms input data.

use serde::{Deserialize, Serialize};

#[link(wasm_import_module = "env")]
extern "C" {
    fn host_log(level: i32, ptr: *const u8, len: usize);
}

fn log_info(msg: &str) {
    unsafe { host_log(1, msg.as_ptr(), msg.len()); }
}

#[derive(Deserialize)]
struct Input {
    {{input_fields}}
}

#[derive(Serialize)]
struct Output {
    {{output_fields}}
}

#[no_mangle]
pub extern "C" fn run(input_ptr: *const u8, input_len: usize) -> u64 {
    let result = run_inner(input_ptr, input_len);
    let json = match result {
        Ok(output) => serde_json::to_string(&output).unwrap_or_else(|e| {
            format!("{{\"error\":\"serialize: {}\"}}", e)
        }),
        Err(e) => format!("{{\"error\":\"{}\"}}", e.replace('"', "'")),
    };
    let bytes = json.into_bytes();
    let ptr = bytes.as_ptr() as u64;
    let len = bytes.len() as u64;
    std::mem::forget(bytes);
    (len << 32) | ptr
}

fn run_inner(input_ptr: *const u8, input_len: usize) -> Result<Output, String> {
    let input_bytes = unsafe { std::slice::from_raw_parts(input_ptr, input_len) };
    let input: Input = serde_json::from_slice(input_bytes)
        .map_err(|e| format!("Invalid input: {}", e))?;

    log_info("Transforming data...");

    {{implementation}}

    Ok(Output {
        {{output_construction}}
    })
}
"##;

pub(super) const WASM_COMPUTE_LIB_RS: &str = r##"//! {{description}}
//!
//! This WASM tool performs pure computation.

use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    {{input_fields}}
}

#[derive(Serialize)]
struct Output {
    {{output_fields}}
}

#[no_mangle]
pub extern "C" fn run(input_ptr: *const u8, input_len: usize) -> u64 {
    let result = run_inner(input_ptr, input_len);
    let json = match result {
        Ok(output) => serde_json::to_string(&output).unwrap_or_else(|e| {
            format!("{{\"error\":\"serialize: {}\"}}", e)
        }),
        Err(e) => format!("{{\"error\":\"{}\"}}", e.replace('"', "'")),
    };
    let bytes = json.into_bytes();
    let ptr = bytes.as_ptr() as u64;
    let len = bytes.len() as u64;
    std::mem::forget(bytes);
    (len << 32) | ptr
}

fn run_inner(input_ptr: *const u8, input_len: usize) -> Result<Output, String> {
    let input_bytes = unsafe { std::slice::from_raw_parts(input_ptr, input_len) };
    let input: Input = serde_json::from_slice(input_bytes)
        .map_err(|e| format!("Invalid input: {}", e))?;

    {{implementation}}

    Ok(Output {
        {{output_construction}}
    })
}
"##;

// =============================================================================
// CLI Templates
// =============================================================================

pub(super) const CLI_CARGO_TOML: &str = r##"[package]
name = "{{name}}"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive"] }
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"##;

pub(super) const CLI_MAIN_RS: &str = r##"//! {{description}}

use clap::Parser;
use anyhow::Result;

#[derive(Parser, Debug)]
#[command(name = "{{name}}")]
#[command(about = "{{description}}")]
struct Args {
    {{cli_args}}
}

fn main() -> Result<()> {
    let args = Args::parse();

    {{implementation}}

    Ok(())
}
"##;

// =============================================================================
// Script Templates
// =============================================================================

pub(super) const PYTHON_SCRIPT: &str = r##"#!/usr/bin/env python3
"""{{description}}"""

import argparse
import json
import sys


def main():
    parser = argparse.ArgumentParser(description="{{description}}")
    {{python_args}}
    args = parser.parse_args()

    {{implementation}}


if __name__ == "__main__":
    main()
"##;

pub(super) const BASH_SCRIPT: &str = r##"#!/bin/bash
# {{description}}

set -euo pipefail

usage() {
    echo "Usage: $0 {{bash_usage}}"
    exit 1
}

{{bash_arg_parsing}}

{{implementation}}
"##;
