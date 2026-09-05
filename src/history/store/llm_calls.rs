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
    /// Optional job UUID linked to this LLM call.
    pub job_id: Option<Uuid>,
    /// Optional conversation UUID linked to this LLM call.
    pub conversation_id: Option<Uuid>,
    /// Identifier for the LLM provider that served the call.
    pub provider: &'a str,
    /// Provider-specific model identifier used for the call.
    pub model: &'a str,
    /// Number of input tokens consumed by the request.
    pub input_tokens: u32,
    /// Number of output tokens returned by the provider.
    pub output_tokens: u32,
    /// Monetary cost recorded for the call.
    pub cost: Decimal,
    /// Optional short description of why the call was made.
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
    //! Unit tests for persisting LLM call records to Postgres.

    use anyhow::Context as _;
    use rstest::{fixture, rstest};

    use super::{LlmCallRecord, Store};
    use crate::testing::postgres::try_test_pg_db;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    #[fixture]
    async fn store() -> anyhow::Result<Option<Store>> {
        let Some(backend) = try_test_pg_db()
            .await
            .context("unexpected Postgres test setup error")?
        else {
            return Ok(None);
        };
        Ok(Some(Store::from_pool(backend.pool())))
    }

    /// Insert the parent rows an attached LLM call needs.
    ///
    /// `llm_calls` carries two foreign keys, `job_id` onto `agent_jobs(id)`
    /// and `conversation_id` onto `conversations(id)`, so a record naming
    /// either can only be written once that parent exists. Both are created
    /// here. Only the columns without a default are supplied; nothing here is
    /// asserted on, because these rows exist to satisfy the constraints rather
    /// than to be the subject of the test.
    async fn insert_parents(store: &Store, job_id: Uuid, conversation_id: Uuid) {
        let conn = store.conn().await.expect("conn should succeed");
        conn.execute(
            r#"
            INSERT INTO conversations (id, channel, user_id)
            VALUES ($1, $2, $3)
            "#,
            &[&conversation_id, &"test", &"llm-call-test"],
        )
        .await
        .expect("insert parent conversations row should succeed");
        conn.execute(
            r#"
            INSERT INTO agent_jobs (id, conversation_id, title, description, status, source)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            &[
                &job_id,
                &conversation_id,
                &"llm call persistence test",
                &"parent job for an attached LLM call",
                &"completed",
                &"test",
            ],
        )
        .await
        .expect("insert parent agent_jobs row should succeed");
    }

    /// Read back a persisted call and assert every column round-trips.
    ///
    /// Shared by both scenarios so the only difference between them is whether
    /// the call names its parents, which is the thing under test. The expected
    /// references are taken from the record itself, so a scenario cannot assert
    /// a value it did not ask for.
    async fn assert_persisted(store: &Store, id: Uuid, record: &LlmCallRecord<'_>) {
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

        assert_eq!(row.get::<_, Option<Uuid>>("job_id"), record.job_id);
        assert_eq!(
            row.get::<_, Option<Uuid>>("conversation_id"),
            record.conversation_id
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
    }

    /// Remove the rows a scenario created, child before parent.
    ///
    /// The call goes first, then the job, then the conversation the job
    /// itself references. Any other order trips the same constraints the
    /// attached scenario exists to exercise.
    async fn cleanup(store: &Store, id: Uuid, parents: Option<(Uuid, Uuid)>) {
        let conn = store.conn().await.expect("conn should succeed");
        conn.execute("DELETE FROM llm_calls WHERE id = $1", &[&id])
            .await
            .expect("delete llm_calls row should succeed");
        if let Some((job_id, conversation_id)) = parents {
            conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&job_id])
                .await
                .expect("delete agent_jobs row should succeed");
            conn.execute(
                "DELETE FROM conversations WHERE id = $1",
                &[&conversation_id],
            )
            .await
            .expect("delete conversations row should succeed");
        }
    }

    /// Persist a call attached to a job and a conversation.
    ///
    /// `llm_calls` references `agent_jobs(id)` and `conversations(id)`, so this
    /// is the only scenario that exercises either constraint: it creates both
    /// parents and then records a call naming them. Written without those rows,
    /// as it was until this split, Postgres rejects the insert on
    /// `llm_calls_job_id_fkey` and the test proves nothing about persistence.
    ///
    /// It is separate from the unattached scenario because the two describe
    /// different contracts. This one says an attached call round-trips and that
    /// its references hold; the other says a call may exist with no job and no
    /// conversation. A single test would have to choose one shape, and would
    /// silently stop covering whichever it did not choose.
    #[rstest]
    #[tokio::test]
    async fn record_llm_call_persists_a_call_attached_to_a_job(
        #[future] store: anyhow::Result<Option<Store>>,
    ) {
        let Some(store) = store.await.expect("store fixture should initialize") else {
            return;
        };
        let job_id = Uuid::new_v4();
        let conversation_id = Uuid::new_v4();
        insert_parents(&store, job_id, conversation_id).await;

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
            .expect("record_llm_call should succeed for an attached call");

        assert_persisted(&store, id, &record).await;
        cleanup(&store, id, Some((job_id, conversation_id))).await;
    }

    /// Persist a call belonging to no job and no conversation.
    ///
    /// Both references are `Option`, so an unattached call is a supported shape
    /// rather than an edge case: a call made outside any job, such as during
    /// interactive use, has no `agent_jobs` row to point at. This scenario pins
    /// that `None` survives the round trip as `NULL`, which the attached
    /// scenario cannot show, and that recording such a call needs no parent
    /// rows at all.
    #[rstest]
    #[tokio::test]
    async fn record_llm_call_persists_a_call_with_no_job(
        #[future] store: anyhow::Result<Option<Store>>,
    ) {
        let Some(store) = store.await.expect("store fixture should initialize") else {
            return;
        };
        let record = LlmCallRecord {
            job_id: None,
            conversation_id: None,
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
            .expect("record_llm_call should succeed for an unattached call");

        assert_persisted(&store, id, &record).await;
        cleanup(&store, id, None).await;
    }
}
