//! Tipe galat terpadu layanan: mengubah galat domain menjadi respons HTTP.

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

/// Galat aplikasi yang dipetakan ke kode status HTTP.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Validasi input gagal (400).
    #[error("validasi gagal: {0}")]
    Validation(String),
    /// Kredensial atau token tidak sah (401).
    #[error("tidak sah")]
    Unauthorized,
    /// Akses ditolak (403).
    #[error("akses ditolak")]
    Forbidden,
    /// Data tidak ditemukan (404).
    #[error("tidak ditemukan")]
    NotFound,
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
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Db(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(json!({ "error": self.to_string() }));
        (status, body).into_response()
    }
}
