#![forbid(unsafe_code)]

//! Versioned, length-framed JSON protocol between Axinite and its Monty
//! helper.
//!
//! The parent process owns every side effect. This crate defines only the wire
//! contract: bounded frames, bounded JSON values, bounded tool schemas and
//! typed terminal outcomes. It never links the interpreter and never performs
//! I/O beyond the caller-supplied readers and writers.
//!
//! # Framing
//!
//! A frame is a four-byte unsigned big-endian length followed by exactly that
//! many bytes of UTF-8 JSON. Zero-length, oversized, truncated and trailing
//! data are rejected before a payload is dispatched. The length ceiling is
//! checked before the payload buffer is allocated.
//!
//! # Versioning
//!
//! [`PROTOCOL_VERSION`] is exchanged in the helper's `hello` and in every
//! `start_run`. A peer that sees any other version must close the channel and
//! report a protocol failure rather than attempting compatibility.

#![deny(missing_docs)]

pub mod frame;
pub mod json;
pub mod messages;
pub mod schema;

pub use frame::{FrameError, MAX_FRAME_BYTES, decode_frame, encode_frame, read_frame, write_frame};
pub use json::{JsonLimitError, MAX_JSON_DEPTH, MAX_STUB_BYTES, validate_json_text};
pub use messages::{
    CallOutcome, CancelRun, DenialCode, GuestLimits, GuestOutput, GuestStream, Hello,
    HelperToParent, HostCall, MAX_DESCRIPTION_BYTES, MAX_IDENTIFIER_BYTES, MAX_RUN_ID_BYTES,
    MAX_TOOLS, ParentToHelper, ResumeRun, StartRun, Terminal, TerminalOutcome, ToolDefinition,
    hello, is_reserved_entrypoint, validate_run_id, validate_start_run,
};
pub use schema::{SchemaError, validate_arguments, validate_schema};

/// The only wire protocol version this crate speaks.
pub const PROTOCOL_VERSION: u32 = 1;
