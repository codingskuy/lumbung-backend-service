//! Ekstraktor bersama: JSON dengan galat baku dan header `X-Perangkat-Id`.

use axum::{
    extract::{rejection::JsonRejection, FromRequest, FromRequestParts, Request},
    http::request::Parts,
    Json,
};
use serde::de::DeserializeOwned;

use crate::error::AppError;

/// Pengganti `axum::Json` yang memetakan galat parsing ke `AppError`:
/// JSON rusak atau bukan JSON menjadi 400, bentuk/tipe salah menjadi 422.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(nilai)) => Ok(ApiJson(nilai)),
            Err(JsonRejection::JsonDataError(e)) => Err(AppError::Unprocessable(e.body_text())),
            Err(e) => Err(AppError::Validation(e.body_text())),
        }
    }
}

/// Header `X-Perangkat-Id` (opsional) untuk audit dan pembatasan per perangkat.
#[derive(Debug, Clone, Default)]
pub struct PerangkatId(pub Option<String>);

impl PerangkatId {
    /// Nilai sebagai `&str` bila ada.
    pub fn sebagai_str(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for PerangkatId {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let nilai = parts
            .headers
            .get("x-perangkat-id")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        Ok(PerangkatId(nilai))
    }
}

/// Deserialisasi bidang opsional yang MENOLAK `null` eksplisit: bidang boleh dihilangkan, tetapi bila
/// ada harus bertipe sesuai skema (skema OpenAPI tidak menandai bidang ini `nullable`).
pub fn opsional_tanpa_null<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}
