//! Estimation snapshot persistence helpers.

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::db::{EstimationActualsParams, EstimationSnapshotParams};
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;
#[cfg(feature = "postgres")]
use uuid::Uuid;

#[cfg(feature = "postgres")]
impl Store {
    /// Save an estimation snapshot for learning.
    pub async fn save_estimation_snapshot(
        &self,
        params: EstimationSnapshotParams<'_>,
    ) -> Result<Uuid, DatabaseError> {
        let EstimationSnapshotParams {
            job_id,
            category,
            tool_names,
            estimated_cost,
            estimated_time_secs,
            estimated_value,
        } = params;
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
        params: EstimationActualsParams,
    ) -> Result<(), DatabaseError> {
        let EstimationActualsParams {
            id,
            actual_cost,
            actual_time_secs,
            actual_value,
        } = params;
        let conn = self.conn().await?;

        let rows = conn.execute(
            "UPDATE estimation_snapshots SET actual_cost = $2, actual_time_secs = $3, actual_value = $4 WHERE id = $1",
            &[&id, &actual_cost, &actual_time_secs, &actual_value],
        )
        .await?;

        if rows == 0 {
            return Err(DatabaseError::NotFound {
                entity: "estimation snapshot".to_string(),
                id: id.to_string(),
            });
        }

        Ok(())
    }
}
