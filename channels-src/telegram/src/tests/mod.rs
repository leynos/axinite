//! Unit tests for the Telegram channel, grouped by concern.
//!
//! Each submodule covers one facet of channel behaviour: attachment and
//! document handling, inbound normalization, polling and webhook transport,
//! outbound messages, status reporting, and API types.

mod attachments;
mod downloads;
mod inbound;
mod polling;
mod send;
mod status;
mod types;
mod webhook;
