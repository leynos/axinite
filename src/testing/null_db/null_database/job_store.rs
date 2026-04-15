//! Null implementation of NativeJobStore for NullDatabase.

use uuid::Uuid;

use crate::context::{ActionRecord, JobContext};
use crate::db::{EstimationActualsParams, EstimationSnapshotParams};
use crate::error::DatabaseError;
use crate::history::{AgentJobRecord, AgentJobSummary, LlmCallRecord};

use super::NullDatabase;

impl crate::db::NativeJobStore for NullDatabase {
    async fn save_job(&self, _ctx: &JobContext) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_job(&self, _id: Uuid) -> Result<Option<JobContext>, DatabaseError> {
        Ok(None)
    }

    async fn update_job_status(
        &self,
        _id: Uuid,
        _status: crate::context::JobState,
        _failure_reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn mark_job_stuck(&self, _id: Uuid) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError> {
        Ok(AgentJobSummary::default())
    }

    async fn get_agent_job_failure_reason(
        &self,
        _id: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        Ok(None)
    }

    async fn save_action(
        &self,
        _job_id: Uuid,
        _action: &ActionRecord,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_job_actions(&self, _job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn record_llm_call(&self, _record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError> {
        self.next_synthetic_uuid()
    }

    async fn save_estimation_snapshot(
        &self,
        _params: EstimationSnapshotParams<'_>,
    ) -> Result<Uuid, DatabaseError> {
        self.next_synthetic_uuid()
    }

    async fn update_estimation_actuals(
        &self,
        _params: EstimationActualsParams,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NativeJobStore;
    use crate::history::LlmCallRecord;

    #[test]
    fn test_synthetic_uuid_is_deterministic() {
        let db = NullDatabase::new();

        let uuid1 = db
            .next_synthetic_uuid()
            .expect("first synthetic UUID generation should succeed");
        let uuid2 = db
            .next_synthetic_uuid()
            .expect("second synthetic UUID generation should succeed");
        let uuid3 = db
            .next_synthetic_uuid()
            .expect("third synthetic UUID generation should succeed");

        // UUIDs should be sequential and unique
        assert_ne!(uuid1, uuid2);
        assert_ne!(uuid2, uuid3);
        assert_ne!(uuid1, uuid3);

        // Each call should increment the counter
        let bytes1 = uuid1.as_bytes();
        let bytes2 = uuid2.as_bytes();
        let bytes3 = uuid3.as_bytes();

        // Convert first 8 bytes back to u128 (big endian)
        let n1 = u128::from_be_bytes(*bytes1);
        let n2 = u128::from_be_bytes(*bytes2);
        let n3 = u128::from_be_bytes(*bytes3);

        assert_eq!(n1 + 1, n2, "Second UUID should be one greater than first");
        assert_eq!(n2 + 1, n3, "Third UUID should be one greater than second");
    }

    #[tokio::test]
    async fn test_record_llm_call_returns_deterministic_uuids() {
        use rust_decimal::Decimal;

        let db = NullDatabase::new();

        let record = LlmCallRecord {
            job_id: Some(Uuid::nil()),
            conversation_id: None,
            provider: "test_provider",
            model: "test",
            input_tokens: 10,
            output_tokens: 20,
            cost: Decimal::ZERO,
            purpose: Some("test"),
        };

        let uuid1 = db
            .record_llm_call(&record)
            .await
            .expect("record_llm_call failed for uuid1");
        let uuid2 = db
            .record_llm_call(&record)
            .await
            .expect("record_llm_call failed for uuid2");

        assert_ne!(uuid1, uuid2, "Each call should return a new UUID");

        // Verify they are sequential
        let n1 = u128::from_be_bytes(*uuid1.as_bytes());
        let n2 = u128::from_be_bytes(*uuid2.as_bytes());
        assert_eq!(n1 + 1, n2, "UUIDs should be sequential");
    }
}
