//! Perakitan router: `/healthz` (infra) dan seluruh API di bawah `/v1` (OpenAPI).

use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};

use crate::{auth::handlers as auth, papan_harga, sistem, state::AppState};

/// Endpoint kesehatan infra: menjawab `ok` hanya bila query ringan ke DB berhasil.
async fn healthz(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({ "status": "ok", "db": "lumbung" })))
}

/// Rute API v1 (relatif terhadap `/v1`).
fn api_v1() -> Router<AppState> {
    Router::new()
        .route("/kesehatan", get(sistem::kesehatan))
        .route("/auth/masuk", post(auth::masuk))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/keluar", post(auth::keluar))
        .route("/gudang/masuk", post(auth::gudang_masuk))
        .route("/gudang/keluar", post(auth::gudang_keluar))
        .route("/papan-harga", get(papan_harga::petani))
        .route("/publik/papan-harga", get(papan_harga::publik))
        .route("/admin/papan-harga", post(papan_harga::terbit))
}

/// Router lengkap layanan.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .nest("/v1", api_v1())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(10),
        ))
        .with_state(state)
}
