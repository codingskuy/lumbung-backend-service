//! Status aplikasi bersama (pool DB + konfigurasi) untuk handler Axum.

use sqlx::PgPool;

use crate::config::Config;

/// Status bersama yang disuntik ke setiap handler via `State<AppState>`.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Pool koneksi Postgres basis data `lumbung`.
    pub pool: PgPool,
    /// Konfigurasi runtime (JWT, API key, alamat dengar).
    pub config: Config,
}

impl AppState {
    /// Bentuk status baru dari pool dan konfigurasi yang sudah tervalidasi.
    pub fn new(pool: PgPool, config: Config) -> Self {
        Self { pool, config }
    }
}
