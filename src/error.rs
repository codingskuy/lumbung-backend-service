//! Tipe galat terpadu layanan: mengubah galat domain menjadi respons HTTP.
//!
//! Pemetaan status mengikuti FSD/30 dan `docs/api/openapi.yaml`:
//! 400 input tak terbaca, 401 tidak sah, 403 ditolak, 404 tidak ditemukan,
//! 409 konflik versi, 422 validasi domain, 429 batas laju/akun terkunci, 500 internal.

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

/// Hasil standar handler.
pub type AppResult<T> = Result<T, AppError>;

/// Galat aplikasi yang dipetakan ke kode status HTTP.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Input tak terbaca atau salah bentuk (400).
    #[error("validasi gagal: {0}")]
    Validation(String),
    /// Validasi domain gagal, contoh total sortir di luar 2 persen (422).
    #[error("validasi gagal: {0}")]
    Unprocessable(String),
    /// Kredensial atau token tidak sah (401).
    #[error("tidak sah")]
    Unauthorized,
    /// Akses ditolak (403).
    #[error("akses ditolak")]
    Forbidden,
    /// Data tidak ditemukan (404).
    #[error("tidak ditemukan")]
    NotFound,
    /// Konflik versi atau keadaan (409).
    #[error("{0}")]
    Conflict(String),
    /// Batas laju terlampaui atau akun terkunci (429).
    #[error("{0}")]
    TooManyRequests(String),
    /// Galat basis data (500).
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    /// Galat internal lain (500).
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            AppError::Validation(_) => StatusCode::BAD_REQUEST,
            AppError::Unprocessable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Db(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        // Detail galat internal hanya masuk log; klien menerima pesan baku.
        let pesan = match &self {
            AppError::Db(e) => {
                tracing::error!(error = %e, "galat basis data");
                "galat internal".to_string()
            }
            AppError::Internal(e) => {
                tracing::error!(error = %e, "galat internal");
                "galat internal".to_string()
            }
            lain => lain.to_string(),
        };
        (status, Json(json!({ "error": pesan }))).into_response()
    }
}
