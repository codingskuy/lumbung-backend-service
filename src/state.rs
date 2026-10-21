//! Status aplikasi bersama (pool DB + konfigurasi) untuk handler Axum.

use std::sync::Arc;

use sqlx::PgPool;

use crate::{auth::jwt::JwtKeys, config::Config};

/// Status bersama yang disuntik ke setiap handler via `State<AppState>`.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Pool koneksi Postgres basis data `lumbung`.
    pub pool: PgPool,
    /// Konfigurasi runtime (JWT, API key, alamat dengar).
    pub config: Config,
    /// Kunci penanda/verifikasi JWT yang sudah di-parse.
    pub jwt: Arc<JwtKeys>,
}

impl AppState {
    /// Bentuk status baru dari pool dan konfigurasi; gagal bila kunci JWT tidak valid.
    pub fn new(pool: PgPool, config: Config) -> anyhow::Result<Self> {
        let jwt = Arc::new(JwtKeys::dari_pem(
            &config.jwt_private_key,
            &config.jwt_public_key,
        )?);
        Ok(Self { pool, config, jwt })
    }
}
