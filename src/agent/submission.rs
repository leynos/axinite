//! Submission types for the turn-based agent loop.
//!
//! Submissions are the different types of input the agent can receive
//! and process as part of the turn-based development loop.
//!
//! Module layout:
//! - [`types`]: the [`Submission`] and [`SubmissionResult`] types
//! - [`parser`]: the [`SubmissionParser`] that classifies raw user input

mod parser;
mod types;

pub use parser::SubmissionParser;
pub use types::{Submission, SubmissionResult};

#[cfg(test)]
mod tests;
