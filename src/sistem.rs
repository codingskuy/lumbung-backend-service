//! Endpoint sistem: `GET /v1/kesehatan` (status ok + waktu server, tanpa login).

use axum::Json;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Serialize;

use crate::waktu;

/// Respons `Kesehatan` pada kontrak.
#[derive(Debug, Serialize)]
pub struct Kesehatan {
    /// Selalu `ok` bila proses hidup.
    pub status: &'static str,
    /// Waktu server dengan offset WIB, contoh `2026-10-04T10:00:00+07:00`.
    pub waktu_server: DateTime<FixedOffset>,
}

/// `GET /v1/kesehatan`: liveness ringan tanpa DB (pemeriksaan DB ada di `/healthz`).
pub async fn kesehatan() -> Json<Kesehatan> {
    Json(Kesehatan {
        status: "ok",
        waktu_server: Utc::now().with_timezone(&waktu::wib()),
    })
}
