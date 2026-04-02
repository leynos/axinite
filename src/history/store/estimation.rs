//! Estimation snapshot persistence helpers.

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;
#[cfg(feature = "postgres")]
use rust_decimal::Decimal;
#[cfg(feature = "postgres")]
use uuid::Uuid;

#[cfg(feature = "postgres")]
impl Store {
    /// Save an estimation snapshot for learning.
    pub async fn save_estimation_snapshot(
        &self,
        job_id: Uuid,
        category: &str,
        tool_names: &[String],
        estimated_cost: Decimal,
        estimated_time_secs: i32,
        estimated_value: Decimal,
    ) -> Result<Uuid, DatabaseError> {
        let conn = self.conn().await?;
        let id = Uuid::new_v4();

        conn.execute(
            r#"
            INSERT INTO estimation_snapshots (id, job_id, category, tool_names, estimated_cost, estimated_time_secs, estimated_value)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            &[
                &id,
                &job_id,
                &category,
                &tool_names,
                &estimated_cost,
                &estimated_time_secs,
                &estimated_value,
            ],
        )
        .await?;

        Ok(id)
    }

    /// Update estimation snapshot with actual values.
    pub async fn update_estimation_actuals(
        &self,
        id: Uuid,
        actual_cost: Decimal,
        actual_time_secs: i32,
        actual_value: Option<Decimal>,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;

        let rows = conn.execute(
            "UPDATE estimation_snapshots SET actual_cost = $2, actual_time_secs = $3, actual_value = $4 WHERE id = $1",
            &[&id, &actual_cost, &actual_time_secs, &actual_value],
        )
        .await?;

        if rows == 0 {
            return Err(DatabaseError::Query(format!(
                "estimation snapshot not found: {id}"
            )));
        }

        Ok(())
    }
}
