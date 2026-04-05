//! Tool-failure persistence helpers.

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::agent::BrokenTool;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
impl Store {
    /// Record a tool failure (upsert: increment count if exists).
    pub async fn record_tool_failure(
        &self,
        tool_name: &str,
        error_message: &str,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;

        conn.execute(
            r#"
            INSERT INTO tool_failures (tool_name, error_message, error_count, last_failure)
            VALUES ($1, $2, 1, NOW())
            ON CONFLICT (tool_name) DO UPDATE SET
                error_message = $2,
                error_count = tool_failures.error_count + 1,
                last_failure = NOW(),
                repaired_at = NULL
            "#,
            &[&tool_name, &error_message],
        )
        .await?;

        Ok(())
    }

    /// Get tools that have failed more than `threshold` times and haven't been repaired.
    pub async fn get_broken_tools(&self, threshold: i32) -> Result<Vec<BrokenTool>, DatabaseError> {
        let conn = self.conn().await?;

        let rows = conn
            .query(
                r#"
                SELECT tool_name, error_message, error_count, first_failure, last_failure,
                       last_build_result, repair_attempts
                FROM tool_failures
                WHERE error_count >= $1 AND repaired_at IS NULL
                ORDER BY error_count DESC
                "#,
                &[&threshold],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|row| BrokenTool {
                name: row.get("tool_name"),
                last_error: row.get("error_message"),
                failure_count: row.get::<_, i32>("error_count") as u32,
                first_failure: row.get("first_failure"),
                last_failure: row.get("last_failure"),
                last_build_result: row.get("last_build_result"),
                repair_attempts: row.get::<_, i32>("repair_attempts") as u32,
            })
            .collect())
    }

    /// Mark a tool as repaired.
    pub async fn mark_tool_repaired(&self, tool_name: &str) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;

        conn.execute(
            "UPDATE tool_failures SET repaired_at = NOW(), error_count = 0 WHERE tool_name = $1",
            &[&tool_name],
        )
        .await?;

        Ok(())
    }

    /// Increment repair attempts for a tool.
    pub async fn increment_repair_attempts(&self, tool_name: &str) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;

        conn.execute(
            "UPDATE tool_failures SET repair_attempts = repair_attempts + 1 WHERE tool_name = $1",
            &[&tool_name],
        )
        .await?;

        Ok(())
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use chrono::Utc;
    use rstest::{fixture, rstest};

    use super::Store;
    use crate::testing::try_test_pg_db;

    #[fixture]
    async fn store() -> Option<Store> {
        let backend = try_test_pg_db().await?;
        Some(Store::from_pool(backend.pool()))
    }

    async fn cleanup_tool(store: &Store, tool_name: &str) {
        let conn = store.conn().await.expect("conn should succeed");
        conn.execute(
            "DELETE FROM tool_failures WHERE tool_name = $1",
            &[&tool_name],
        )
        .await
        .expect("delete tool_failures should succeed");
    }

    #[rstest]
    #[tokio::test]
    async fn record_tool_failure_inserts_and_upserts(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let tool_name = format!("broken-tool-{}", uuid::Uuid::new_v4());

        store
            .record_tool_failure(&tool_name, "first failure")
            .await
            .expect("first record_tool_failure should succeed");
        store
            .record_tool_failure(&tool_name, "second failure")
            .await
            .expect("second record_tool_failure should succeed");

        let conn = store.conn().await.expect("conn should succeed");
        let row = conn
            .query_one(
                "SELECT error_message, error_count, repaired_at FROM tool_failures WHERE tool_name = $1",
                &[&tool_name],
            )
            .await
            .expect("query tool_failures should succeed");

        assert_eq!(row.get::<_, String>("error_message"), "second failure");
        assert_eq!(row.get::<_, i32>("error_count"), 2);
        assert_eq!(
            row.get::<_, Option<chrono::DateTime<Utc>>>("repaired_at"),
            None
        );

        cleanup_tool(&store, &tool_name).await;
    }

    #[rstest]
    #[tokio::test]
    async fn get_broken_tools_filters_by_threshold_and_repaired_at(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let eligible_tool = format!("eligible-tool-{}", uuid::Uuid::new_v4());
        let repaired_tool = format!("repaired-tool-{}", uuid::Uuid::new_v4());
        let below_threshold_tool = format!("below-tool-{}", uuid::Uuid::new_v4());
        let conn = store.conn().await.expect("conn should succeed");

        conn.execute(
            r#"
            INSERT INTO tool_failures (tool_name, error_message, error_count, first_failure, last_failure, repaired_at, repair_attempts)
            VALUES ($1, $2, $3, NOW(), NOW(), NULL, 0),
                   ($4, $5, $6, NOW(), NOW(), NOW(), 0),
                   ($7, $8, $9, NOW(), NOW(), NULL, 0)
            "#,
            &[
                &eligible_tool,
                &"eligible failure",
                &3i32,
                &repaired_tool,
                &"repaired failure",
                &5i32,
                &below_threshold_tool,
                &"below threshold failure",
                &1i32,
            ],
        )
        .await
        .expect("seed tool_failures should succeed");

        let broken = store
            .get_broken_tools(3)
            .await
            .expect("get_broken_tools should succeed");

        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].name, eligible_tool);

        cleanup_tool(&store, &eligible_tool).await;
        cleanup_tool(&store, &repaired_tool).await;
        cleanup_tool(&store, &below_threshold_tool).await;
    }

    #[rstest]
    #[tokio::test]
    async fn mark_tool_repaired_sets_repaired_at_and_resets_error_count(
        #[future] store: Option<Store>,
    ) {
        let Some(store) = store.await else { return };
        let tool_name = format!("repairable-tool-{}", uuid::Uuid::new_v4());

        store
            .record_tool_failure(&tool_name, "failure")
            .await
            .expect("record_tool_failure should succeed");
        store
            .mark_tool_repaired(&tool_name)
            .await
            .expect("mark_tool_repaired should succeed");

        let conn = store.conn().await.expect("conn should succeed");
        let row = conn
            .query_one(
                "SELECT error_count, repaired_at FROM tool_failures WHERE tool_name = $1",
                &[&tool_name],
            )
            .await
            .expect("query tool_failures should succeed");

        assert_eq!(row.get::<_, i32>("error_count"), 0);
        assert!(
            row.get::<_, Option<chrono::DateTime<Utc>>>("repaired_at")
                .is_some()
        );

        cleanup_tool(&store, &tool_name).await;
    }

    #[rstest]
    #[tokio::test]
    async fn increment_repair_attempts_increments_by_one(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let tool_name = format!("repair-attempt-tool-{}", uuid::Uuid::new_v4());

        store
            .record_tool_failure(&tool_name, "failure")
            .await
            .expect("record_tool_failure should succeed");
        store
            .increment_repair_attempts(&tool_name)
            .await
            .expect("increment_repair_attempts should succeed");

        let conn = store.conn().await.expect("conn should succeed");
        let row = conn
            .query_one(
                "SELECT repair_attempts FROM tool_failures WHERE tool_name = $1",
                &[&tool_name],
            )
            .await
            .expect("query tool_failures should succeed");

        assert_eq!(row.get::<_, i32>("repair_attempts"), 1);

        cleanup_tool(&store, &tool_name).await;
    }
}
