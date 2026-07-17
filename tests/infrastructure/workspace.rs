//! Integration tests for the workspace module.
//!
//! Requires a running PostgreSQL with pgvector extension.
//! Set DATABASE_URL=postgres://localhost/axinite_test

#![cfg(feature = "postgres")]

#[path = "workspace/files.rs"]
mod files;
#[path = "workspace/memory_and_search.rs"]
mod memory_and_search;

fn get_pool() -> deadpool_postgres::Pool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost/axinite_test".to_string());

    let config: tokio_postgres::Config = database_url.parse().expect("Invalid DATABASE_URL");

    let mgr = deadpool_postgres::Manager::new(config, tokio_postgres::NoTls);
    deadpool_postgres::Pool::builder(mgr)
        .max_size(4)
        .build()
        .expect("Failed to create pool")
}

/// Try to get a connection, returning None if Postgres is unreachable.
/// Tests call this to skip gracefully in CI where no database is available.
async fn try_connect(pool: &deadpool_postgres::Pool) -> Option<()> {
    match pool.get().await {
        Ok(_) => Some(()),
        Err(e) => {
            eprintln!("skipping: database unavailable ({e})");
            None
        }
    }
}

async fn cleanup_user(pool: &deadpool_postgres::Pool, user_id: &str) {
    let conn = pool.get().await.expect("Failed to get connection");
    conn.execute(
        "DELETE FROM memory_documents WHERE user_id = $1",
        &[&user_id],
    )
    .await
    .ok();
}
