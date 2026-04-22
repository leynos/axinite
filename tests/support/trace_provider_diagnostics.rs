//! Optional `TraceLlm` diagnostics used by support-unit tests.

use std::path::Path;
use std::sync::atomic::Ordering;

use super::{trace_provider::TraceLlm, trace_types::LlmTrace};

impl TraceLlm {
    /// Load from a JSON file and create the provider.
    pub async fn from_file_async(
        path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let trace = LlmTrace::from_file_async(path).await?;
        Ok(Self::from_trace(trace))
    }

    /// Number of calls made so far.
    pub fn calls(&self) -> usize {
        self.inner
            .lock()
            .map(|inner| inner.index)
            .unwrap_or_else(|poisoned| poisoned.into_inner().index)
    }

    /// Number of request-hint mismatches observed (warnings only).
    pub fn hint_mismatches(&self) -> usize {
        self.hint_mismatches.load(Ordering::Relaxed)
    }
}
