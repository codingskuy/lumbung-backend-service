//! Titik masuk layanan backend Lumbung (Axum).
//!
//! Menjalankan migrasi, mendaftarkan kunci perangkat gudang dari environment,
//! lalu melayani router dari pustaka (`/healthz` dan API di bawah `/v1`).

use lumbung_backend_service::{auth::kunci, config::Config, db, router, state::AppState};

/// Label kunci perangkat gudang yang berasal dari `DEVICE_API_KEY_GUDANG`.
const LABEL_GUDANG: &str = "GUDANG";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    kunci::pastikan_kunci_awal(&pool, LABEL_GUDANG, &config.device_api_key).await?;

    let state = AppState::new(pool, config)?;
    let app = router(state.clone());

    let listener = tokio::net::TcpListener::bind(&state.config.listen_addr).await?;
    tracing::info!(
        "lumbung-backend-service menyala di {}",
        state.config.listen_addr
    );
    axum::serve(listener, app).await?;
    Ok(())
}
