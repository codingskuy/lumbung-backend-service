//! Titik masuk layanan backend Lumbung (Axum).
//!
//! Menyediakan router HTTP dan endpoint `GET /healthz` yang memeriksa
//! konektivitas basis data `lumbung` sebelum menjawab `ok`.

mod config;
mod db;
mod error;
mod state;

use std::time::Duration;

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde_json::{json, Value};
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};

use state::AppState;

/// Endpoint kesehatan: menjawab `ok` hanya bila query ringan ke DB berhasil.
async fn healthz(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({ "status": "ok", "db": "lumbung" })))
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(TimeoutLayer::new(Duration::from_secs(10)))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = config::Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let state = AppState::new(pool, config);
    let app = router(state.clone());

    let listener = tokio::net::TcpListener::bind(&state.config.listen_addr).await?;
    tracing::info!("lumbung-backend-service menyala di {}", state.config.listen_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
