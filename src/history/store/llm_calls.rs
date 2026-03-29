//! LLM call persistence types and helpers.

use rust_decimal::Decimal;
use uuid::Uuid;

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

/// Record for an LLM call to be persisted.
#[derive(Debug, Clone)]
pub struct LlmCallRecord<'a> {
    pub job_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub provider: &'a str,
    pub model: &'a str,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost: Decimal,
    pub purpose: Option<&'a str>,
}

#[cfg(feature = "postgres")]
impl Store {
    /// Record an LLM call.
    pub async fn record_llm_call(&self, record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError> {
        let conn = self.conn().await?;
        let id = Uuid::new_v4();

        conn.execute(
            r#"
            INSERT INTO llm_calls (id, job_id, conversation_id, provider, model, input_tokens, output_tokens, cost, purpose)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            &[
                &id,
                &record.job_id,
                &record.conversation_id,
                &record.provider,
                &record.model,
                &(record.input_tokens as i32),
                &(record.output_tokens as i32),
                &record.cost,
                &record.purpose,
            ],
        )
        .await?;

        Ok(id)
    }
}
