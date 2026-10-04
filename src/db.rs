//! Koneksi basis data Postgres `lumbung` memakai `sqlx`.
//!
//! Pool disetel di sini; tuning lanjutan ada di runbook infra.

use sqlx::postgres::{PgPool, PgPoolOptions};

/// Buka pool koneksi ke basis data `lumbung`.
///
/// Batas default mengikuti runbook: `max_connections = 2 × vCPU` per replika.
/// Panggil dengan `DATABASE_URL` basis data `lumbung` (terpisah dari divisi lain).
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .min_connections(2)
        .max_connections(8)
        .idle_timeout(std::time::Duration::from_secs(300))
        .max_lifetime(std::time::Duration::from_secs(1800))
        .connect(database_url)
        .await?;
    Ok(pool)
}
