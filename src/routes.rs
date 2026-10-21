//! Perakitan router: `/healthz` (infra) dan seluruh API di bawah `/v1` (OpenAPI).

use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};

use crate::{
    admin, auth::handlers as auth, error::AppError, invoice, keranjang, lot, papan_harga,
    pembayaran, sinkron, sistem, sopir, state::AppState, trip,
};

/// Endpoint kesehatan infra: menjawab `ok` hanya bila query ringan ke DB berhasil.
async fn healthz(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({ "status": "ok", "db": "lumbung" })))
}

/// Fallback JSON baku untuk rute tak dikenal (404, `Galat`).
async fn rute_tak_dikenal() -> AppError {
    AppError::NotFound
}

/// Fallback JSON baku untuk metode tak didukung pada rute yang ada (405, `Galat`).
async fn metode_tak_didukung() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(json!({ "error": "metode tidak didukung" })),
    )
        .into_response()
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
        .route("/lot", post(lot::buat))
        .route("/lot/{id}/sortir", post(lot::sortir))
        .route("/keranjang/batch", post(keranjang::batch))
        .route("/stok-lot", get(keranjang::stok_lot))
        .route("/trip", post(trip::buat))
        .route("/trip/{id}/surat-jalan", post(trip::terbit_surat_jalan))
        .route("/trip-hari-ini", get(trip::trip_hari_ini))
        .route("/sinkron/batch", post(sinkron::batch))
        .merge(sopir::rute())
        .route("/pembayaran-saya", get(pembayaran::saya))
        .route("/admin/invoice", get(invoice::daftar))
        .route("/admin/invoice/{id}/lunas", post(invoice::lunas))
        .route("/admin/rekap-harian", get(admin::rekap_harian))
        .route("/admin/mitra", get(admin::mitra))
        .route("/admin/mitra/{id}/status", post(admin::mitra_status))
        .route("/admin/insiden", get(admin::insiden))
        .route("/publik/profil", get(admin::profil))
        .route("/kontrak-saya", get(admin::kontrak_saya))
        .route("/rencana-setoran", post(admin::rencana_setoran))
        .route("/papan-harga", get(papan_harga::petani))
        .route("/publik/papan-harga", get(papan_harga::publik))
        .route("/admin/papan-harga", post(papan_harga::terbit))
}

/// Router lengkap layanan.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .nest(
            "/v1",
            api_v1()
                .fallback(rute_tak_dikenal)
                .method_not_allowed_fallback(metode_tak_didukung),
        )
        .fallback(rute_tak_dikenal)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(10),
        ))
        .with_state(state)
}
