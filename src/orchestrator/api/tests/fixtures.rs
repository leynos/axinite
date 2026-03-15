//! Shared test fixtures for orchestrator API endpoint tests.

use std::collections::HashMap;
use std::sync::Arc;

use rstest::fixture;
use tokio::sync::Mutex;

use super::*;

#[fixture]
pub(super) fn test_state() -> OrchestratorState {
    let token_store = TokenStore::new();
    let jm = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    OrchestratorState {
        llm: Arc::new(StubLlm::default()),
        tools: Arc::new(ToolRegistry::new()),
        job_manager: Arc::new(jm),
        token_store,
        job_event_tx: None,
        prompt_queue: Arc::new(Mutex::new(HashMap::new())),
        store: None,
        secrets_store: None,
        user_id: "default".to_string(),
    }
}
