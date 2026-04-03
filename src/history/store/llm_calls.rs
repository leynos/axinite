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
        let input_tokens = i64::from(record.input_tokens);
        let output_tokens = i64::from(record.output_tokens);

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
                &input_tokens,
                &output_tokens,
                &record.cost,
                &record.purpose,
            ],
        )
        .await?;

        Ok(id)
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use rstest::{fixture, rstest};

    use super::{LlmCallRecord, Store};
    use crate::testing::try_test_pg_db;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    #[fixture]
    async fn store() -> Option<Store> {
        let backend = try_test_pg_db().await?;
        Some(Store::from_pool(backend.pool()))
    }

    #[rstest]
    #[tokio::test]
    async fn record_llm_call_persists_expected_values(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let job_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        let record = LlmCallRecord {
            job_id: Some(job_id),
            conversation_id: Some(conversation_id),
            provider: "nearai",
            model: "test-model",
            input_tokens: 1234,
            output_tokens: 567,
            cost: Decimal::new(123, 2),
            purpose: Some("integration-test"),
        };

        let id = store
            .record_llm_call(&record)
            .await
            .expect("record_llm_call should succeed");

        let conn = store.conn().await.expect("conn should succeed");
        let row = conn
            .query_one(
                r#"
                SELECT job_id, conversation_id, provider, model, input_tokens, output_tokens, cost, purpose
                FROM llm_calls
                WHERE id = $1
                "#,
                &[&id],
            )
            .await
            .expect("query llm_calls row should succeed");

        assert_eq!(row.get::<_, Option<Uuid>>("job_id"), Some(job_id));
        assert_eq!(
            row.get::<_, Option<Uuid>>("conversation_id"),
            Some(conversation_id)
        );
        assert_eq!(row.get::<_, String>("provider"), record.provider);
        assert_eq!(row.get::<_, String>("model"), record.model);
        assert_eq!(
            row.get::<_, i64>("input_tokens"),
            i64::from(record.input_tokens)
        );
        assert_eq!(
            row.get::<_, i64>("output_tokens"),
            i64::from(record.output_tokens)
        );
        assert_eq!(row.get::<_, rust_decimal::Decimal>("cost"), record.cost);
        assert_eq!(
            row.get::<_, Option<String>>("purpose"),
            record.purpose.map(String::from)
        );

        conn.execute("DELETE FROM llm_calls WHERE id = $1", &[&id])
            .await
            .expect("delete llm_calls row should succeed");
    }
}
