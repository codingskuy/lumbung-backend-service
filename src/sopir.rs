//! Endpoint sopir (LMB-BE-005): tiba/berangkat titik, bukti terima, unggah foto, insiden.
//!
//! Sopir hanya boleh menyentuh trip yang ditugaskan kepadanya (403 + audit bila tidak). Semua POST
//! idempoten via UUID klien. Resolusi konflik (FSD/10): lapangan menang untuk bukti terima dan posisi,
//! gudang menang untuk berat dan grade; konflik dicatat sebagai insiden `konflik_sinkron`.

use std::collections::HashSet;

use axum::{
    extract::{multipart::MultipartRejection, DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    audit,
    auth::principal::{Pengguna, PeranSopir},
    domain::{AksiHasil, Hasil, Konteks},
    error::{AppError, AppResult},
    extract::ApiJson,
    state::AppState,
    trip::cari_trip,
    urutan, waktu,
};

/// Ukuran foto maksimal: 500 KB (dihitung 500 x 1024 byte).
pub const MAKS_UKURAN_FOTO: usize = 500 * 1024;
/// Foto maksimal per bukti terima atau insiden.
pub const MAKS_FOTO_PER_BUKTI: usize = 4;
/// Batas badan permintaan unggah foto (di atas 500 KB agar pelanggaran ukuran menjadi 422).
const BATAS_BADAN_UNGGAH: usize = 1024 * 1024;

/// Badan `POST /titik/{id}/tiba` dan `/berangkat`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitikRequest {
    /// UUID klien.
    pub uuid: Uuid,
    /// Kode trip.
    pub trip_id: String,
    /// Kode titik (harus sama dengan `{id}` pada path).
    pub titik_id: String,
    /// Waktu kejadian (RFC 3339 dengan offset).
    pub waktu: DateTime<Utc>,
    /// Lintang.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub lat: Option<f64>,
    /// Bujur.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub lng: Option<f64>,
}

/// Badan `POST /bukti-terima`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuktiTerimaRequest {
    /// UUID klien.
    pub uuid: Uuid,
    /// Kode trip.
    pub trip_id: String,
    /// Kode titik.
    pub titik_id: String,
    /// Waktu serah terima.
    pub waktu: DateTime<Utc>,
    /// ID foto hasil `POST /unggah-foto`, maksimal 4.
    pub foto_ids: Vec<Uuid>,
    /// ID berkas tanda tangan penerima (opsional).
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub tanda_tangan: Option<String>,
    /// Catatan bebas.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub catatan: Option<String>,
}

/// Badan `POST /insiden`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InsidenRequest {
    /// UUID klien.
    pub uuid: Uuid,
    /// Jenis insiden, contoh `keterlambatan`.
    pub jenis: String,
    /// Catatan bebas.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub catatan: Option<String>,
    /// Foto pendukung, maksimal 4.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub foto_ids: Option<Vec<Uuid>>,
}

/// Jenis kejadian titik.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JenisTitik {
    /// Tiba di titik.
    Tiba,
    /// Berangkat dari titik.
    Berangkat,
}

impl JenisTitik {
    fn teks(self) -> &'static str {
        match self {
            JenisTitik::Tiba => "TIBA",
            JenisTitik::Berangkat => "BERANGKAT",
        }
    }
}

fn aksi(id: String, duplikat: bool) -> Response {
    let status = if duplikat {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    (status, Json(AksiHasil { id, duplikat })).into_response()
}

/// Pastikan trip ada dan ditugaskan ke sopir ini; selain itu 404 atau 403 (dicatat di audit).
async fn pastikan_penugasan(
    state: &AppState,
    pengguna: &Pengguna,
    trip_kode: &str,
) -> AppResult<()> {
    let sopir: Option<Option<String>> =
        sqlx::query_scalar("SELECT sopir_akun FROM trip WHERE kode = $1")
            .bind(trip_kode)
            .fetch_optional(&state.pool)
            .await?;
    match sopir {
        None => Err(AppError::NotFound),
        Some(Some(akun)) if akun == pengguna.akun => Ok(()),
        Some(_) => {
            let mut conn = state.pool.acquire().await?;
            audit::catat(
                &mut conn,
                &pengguna.akun,
                "akses_ditolak",
                pengguna.perangkat_id.as_deref(),
                json!({"alasan": "trip_bukan_milik_sopir", "trip": trip_kode}),
            )
            .await?;
            Err(AppError::Forbidden)
        }
    }
}

/// Catat konflik sinkron sebagai insiden `konflik_sinkron` (dasar log konflik, FSD/10).
pub async fn catat_konflik(
    conn: &mut PgConnection,
    ctx: &Konteks,
    catatan: &str,
) -> AppResult<String> {
    let tanggal = waktu::hari_ini_wib();
    let urut = urutan::berikutnya(&mut *conn, "INS", tanggal).await?;
    let kode = urutan::format_nomor("INS", tanggal, urut);
    sqlx::query(
        "INSERT INTO insiden (kode, uuid_klien, jenis, catatan, dicatat_oleh)
         VALUES ($1, $2, 'konflik_sinkron', $3, $4)",
    )
    .bind(&kode)
    .bind(Uuid::now_v7())
    .bind(catatan)
    .bind(&ctx.akun)
    .execute(conn)
    .await?;
    Ok(kode)
}

fn validasi_titik(path_id: &str, req: &TitikRequest) -> AppResult<()> {
    if path_id != req.titik_id {
        return Err(AppError::Unprocessable(
            "titik_id pada badan berbeda dengan {id} pada path".into(),
        ));
    }
    if req.lat.is_some_and(|v| !(-90.0..=90.0).contains(&v)) {
        return Err(AppError::Unprocessable(
            "lat harus antara -90 dan 90".into(),
        ));
    }
    if req.lng.is_some_and(|v| !(-180.0..=180.0).contains(&v)) {
        return Err(AppError::Unprocessable(
            "lng harus antara -180 dan 180".into(),
        ));
    }
    Ok(())
}

/// Catat tiba/berangkat di dalam transaksi pemanggil. Kejadian ulang dengan UUID berbeda
/// menimpa waktu (lapangan menang untuk posisi). Trip menjadi `BERJALAN` pada tiba pertama dan
/// `SELESAI` saat semua titik sudah berangkat.
pub async fn catat_titik_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    jenis: JenisTitik,
    path_id: &str,
    req: &TitikRequest,
) -> AppResult<Hasil<Uuid>> {
    validasi_titik(path_id, req)?;
    let ada: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM titik_event WHERE uuid_klien = $1)")
            .bind(req.uuid)
            .fetch_one(&mut *conn)
            .await?;
    if ada {
        return Ok(Hasil::Duplikat(req.uuid.to_string()));
    }
    let trip = cari_trip(conn, &req.trip_id, true)
        .await?
        .ok_or(AppError::NotFound)?;
    let titik: Option<(Option<DateTime<Utc>>,)> = sqlx::query_as(
        "SELECT tiba_at FROM trip_titik WHERE trip_id = $1 AND titik_id = $2 FOR UPDATE",
    )
    .bind(trip.id)
    .bind(&req.titik_id)
    .fetch_optional(&mut *conn)
    .await?;
    let (tiba_at,) = titik.ok_or(AppError::NotFound)?;

    match jenis {
        JenisTitik::Tiba => {
            sqlx::query("UPDATE trip_titik SET tiba_at = $3 WHERE trip_id = $1 AND titik_id = $2")
                .bind(trip.id)
                .bind(&req.titik_id)
                .bind(req.waktu)
                .execute(&mut *conn)
                .await?;
            if trip.status == "DISUSUN" {
                sqlx::query(
                    "UPDATE trip SET status = 'BERJALAN', versi = versi + 1, diperbarui_at = now() WHERE id = $1",
                )
                .bind(trip.id)
                .execute(&mut *conn)
                .await?;
            }
        }
        JenisTitik::Berangkat => {
            let tiba = tiba_at.ok_or_else(|| {
                AppError::Unprocessable(format!("belum tiba di titik {}", req.titik_id))
            })?;
            if req.waktu < tiba {
                return Err(AppError::Unprocessable(
                    "waktu berangkat lebih awal dari waktu tiba".into(),
                ));
            }
            sqlx::query(
                "UPDATE trip_titik SET berangkat_at = $3 WHERE trip_id = $1 AND titik_id = $2",
            )
            .bind(trip.id)
            .bind(&req.titik_id)
            .bind(req.waktu)
            .execute(&mut *conn)
            .await?;
            let belum: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM trip_titik WHERE trip_id = $1 AND berangkat_at IS NULL",
            )
            .bind(trip.id)
            .fetch_one(&mut *conn)
            .await?;
            if belum == 0 && trip.status != "SELESAI" {
                selesaikan_trip(conn, trip.id).await?;
            }
        }
    }
    sqlx::query(
        "INSERT INTO titik_event (uuid_klien, trip_id, titik_id, jenis, waktu, lat, lng, dicatat_oleh)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(req.uuid)
    .bind(trip.id)
    .bind(&req.titik_id)
    .bind(jenis.teks())
    .bind(req.waktu)
    .bind(req.lat)
    .bind(req.lng)
    .bind(&ctx.akun)
    .execute(&mut *conn)
    .await?;
    Ok(Hasil::Baru(req.uuid))
}

/// Tandai trip selesai dan kosongkan keranjang yang belum selesai (keranjang fisik kembali dipakai).
async fn selesaikan_trip(conn: &mut PgConnection, trip_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "UPDATE trip SET status = 'SELESAI', versi = versi + 1, diperbarui_at = now() WHERE id = $1",
    )
    .bind(trip_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE keranjang_isi SET selesai_at = now() WHERE trip_id = $1 AND selesai_at IS NULL",
    )
    .bind(trip_id)
    .execute(&mut *conn)
    .await?;
    perbarui_lot_diterima(conn, trip_id).await
}

/// Lot menjadi `DITERIMA` bila seluruh isi keranjangnya sudah selesai (versi naik).
async fn perbarui_lot_diterima(conn: &mut PgConnection, trip_id: Uuid) -> AppResult<()> {
    sqlx::query(
        "UPDATE lot SET status = 'DITERIMA', versi = versi + 1, diperbarui_at = now()
         WHERE status = 'DIMUAT'
           AND id IN (SELECT lot_id FROM keranjang_isi WHERE trip_id = $1)
           AND NOT EXISTS (SELECT 1 FROM keranjang_isi k WHERE k.lot_id = lot.id AND k.selesai_at IS NULL)",
    )
    .bind(trip_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Validasi murni daftar foto (jumlah dan dobel); dilakukan SEBELUM pencarian idempotensi agar
/// permintaan yang melanggar skema tidak diterima diam-diam sebagai duplikat.
fn validasi_foto_ids(foto_ids: &[Uuid]) -> AppResult<()> {
    if foto_ids.len() > MAKS_FOTO_PER_BUKTI {
        return Err(AppError::Unprocessable(format!(
            "foto maksimal {MAKS_FOTO_PER_BUKTI} per bukti"
        )));
    }
    let unik: HashSet<_> = foto_ids.iter().collect();
    if unik.len() != foto_ids.len() {
        return Err(AppError::Unprocessable("foto_ids dobel".into()));
    }
    Ok(())
}

async fn pastikan_foto_ada(conn: &mut PgConnection, foto_ids: &[Uuid]) -> AppResult<()> {
    validasi_foto_ids(foto_ids)?;
    if foto_ids.is_empty() {
        return Ok(());
    }
    let ada: i64 = sqlx::query_scalar("SELECT count(*) FROM foto WHERE id = ANY($1)")
        .bind(foto_ids)
        .fetch_one(conn)
        .await?;
    if ada != foto_ids.len() as i64 {
        return Err(AppError::Unprocessable(
            "ada foto yang belum diunggah lewat /unggah-foto".into(),
        ));
    }
    Ok(())
}

/// Catat bukti terima di dalam transaksi pemanggil. Bukti terbaru menggantikan yang lama
/// (lapangan menang) dan penggantian dicatat sebagai insiden `konflik_sinkron`.
pub async fn bukti_terima_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    req: &BuktiTerimaRequest,
) -> AppResult<Hasil<Uuid>> {
    if req.foto_ids.is_empty()
        && req
            .tanda_tangan
            .as_deref()
            .is_none_or(|t| t.trim().is_empty())
    {
        return Err(AppError::Unprocessable(
            "bukti terima butuh minimal satu foto atau tanda tangan".into(),
        ));
    }
    if req.tanda_tangan.as_deref().is_some_and(|t| t.len() > 128)
        || req.catatan.as_deref().is_some_and(|c| c.len() > 2000)
    {
        return Err(AppError::Unprocessable(
            "tanda_tangan maksimal 128 dan catatan maksimal 2000 karakter".into(),
        ));
    }
    validasi_foto_ids(&req.foto_ids)?;
    let ada: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM bukti_terima WHERE uuid_klien = $1)")
            .bind(req.uuid)
            .fetch_one(&mut *conn)
            .await?;
    if ada {
        return Ok(Hasil::Duplikat(req.uuid.to_string()));
    }
    pastikan_foto_ada(conn, &req.foto_ids).await?;
    let trip = cari_trip(conn, &req.trip_id, true)
        .await?
        .ok_or(AppError::NotFound)?;
    let titik_ada: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM trip_titik WHERE trip_id = $1 AND titik_id = $2)",
    )
    .bind(trip.id)
    .bind(&req.titik_id)
    .fetch_one(&mut *conn)
    .await?;
    if !titik_ada {
        return Err(AppError::NotFound);
    }
    let sebelumnya: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bukti_terima WHERE trip_id = $1 AND titik_id = $2",
    )
    .bind(trip.id)
    .bind(&req.titik_id)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO bukti_terima (uuid_klien, trip_id, titik_id, waktu, foto_ids, tanda_tangan, catatan, dicatat_oleh)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(req.uuid)
    .bind(trip.id)
    .bind(&req.titik_id)
    .bind(req.waktu)
    .bind(&req.foto_ids)
    .bind(&req.tanda_tangan)
    .bind(&req.catatan)
    .bind(&ctx.akun)
    .execute(&mut *conn)
    .await?;
    if sebelumnya > 0 {
        catat_konflik(
            conn,
            ctx,
            &format!(
                "bukti terima {} titik {} diganti bukti terbaru dari lapangan (lapangan menang)",
                trip.kode, req.titik_id
            ),
        )
        .await?;
    }
    sqlx::query(
        "UPDATE keranjang_isi SET selesai_at = now()
         WHERE trip_id = $1 AND titik_id = $2 AND selesai_at IS NULL",
    )
    .bind(trip.id)
    .bind(&req.titik_id)
    .execute(&mut *conn)
    .await?;
    perbarui_lot_diterima(conn, trip.id).await?;
    Ok(Hasil::Baru(req.uuid))
}

/// Catat insiden di dalam transaksi pemanggil; kode `INS-YYYYMMDD-NNN`.
pub async fn insiden_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    req: &InsidenRequest,
) -> AppResult<Hasil<String>> {
    let jenis = req.jenis.trim();
    if jenis.is_empty() || jenis.len() > 64 {
        return Err(AppError::Unprocessable(
            "jenis insiden wajib diisi (maksimal 64 karakter)".into(),
        ));
    }
    if req.catatan.as_deref().is_some_and(|c| c.len() > 2000) {
        return Err(AppError::Unprocessable(
            "catatan maksimal 2000 karakter".into(),
        ));
    }
    validasi_foto_ids(req.foto_ids.as_deref().unwrap_or_default())?;
    let ada: Option<String> = sqlx::query_scalar("SELECT kode FROM insiden WHERE uuid_klien = $1")
        .bind(req.uuid)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(kode) = ada {
        return Ok(Hasil::Duplikat(kode));
    }
    let foto = req.foto_ids.clone().unwrap_or_default();
    pastikan_foto_ada(conn, &foto).await?;
    let tanggal = waktu::hari_ini_wib();
    let urut = urutan::berikutnya(&mut *conn, "INS", tanggal).await?;
    let kode = urutan::format_nomor("INS", tanggal, urut);
    sqlx::query(
        "INSERT INTO insiden (kode, uuid_klien, jenis, catatan, foto_ids, dicatat_oleh)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&kode)
    .bind(req.uuid)
    .bind(jenis)
    .bind(&req.catatan)
    .bind(&foto)
    .bind(&ctx.akun)
    .execute(conn)
    .await?;
    Ok(Hasil::Baru(kode))
}

async fn titik_handler(
    state: AppState,
    pengguna: Pengguna,
    jenis: JenisTitik,
    id: String,
    req: TitikRequest,
) -> AppResult<Response> {
    pastikan_penugasan(&state, &pengguna, &req.trip_id).await?;
    let ctx = pengguna.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = catat_titik_tx(&mut tx, &ctx, jenis, &id, &req).await?;
    tx.commit().await?;
    Ok(match hasil {
        Hasil::Baru(u) => aksi(u.to_string(), false),
        Hasil::Duplikat(u) => aksi(u, true),
    })
}

/// `POST /titik/{id}/tiba`.
pub async fn titik_tiba(
    State(state): State<AppState>,
    PeranSopir(pengguna): PeranSopir,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<TitikRequest>,
) -> AppResult<Response> {
    titik_handler(state, pengguna, JenisTitik::Tiba, id, req).await
}

/// `POST /titik/{id}/berangkat`.
pub async fn titik_berangkat(
    State(state): State<AppState>,
    PeranSopir(pengguna): PeranSopir,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<TitikRequest>,
) -> AppResult<Response> {
    titik_handler(state, pengguna, JenisTitik::Berangkat, id, req).await
}

/// `POST /bukti-terima`.
pub async fn bukti_terima(
    State(state): State<AppState>,
    PeranSopir(pengguna): PeranSopir,
    ApiJson(req): ApiJson<BuktiTerimaRequest>,
) -> AppResult<Response> {
    pastikan_penugasan(&state, &pengguna, &req.trip_id).await?;
    let ctx = pengguna.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = bukti_terima_tx(&mut tx, &ctx, &req).await?;
    tx.commit().await?;
    Ok(match hasil {
        Hasil::Baru(u) => aksi(u.to_string(), false),
        Hasil::Duplikat(u) => aksi(u, true),
    })
}

/// `POST /insiden`.
pub async fn insiden(
    State(state): State<AppState>,
    PeranSopir(pengguna): PeranSopir,
    ApiJson(req): ApiJson<InsidenRequest>,
) -> AppResult<Response> {
    let ctx = pengguna.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = insiden_tx(&mut tx, &ctx, &req).await?;
    tx.commit().await?;
    Ok(match hasil {
        Hasil::Baru(k) => aksi(k, false),
        Hasil::Duplikat(k) => aksi(k, true),
    })
}

/// Berkas sah bila JPEG utuh: diawali `FF D8 FF` dan diakhiri `FF D9`.
pub fn jpeg_valid(data: &[u8]) -> bool {
    data.len() >= 4 && data[..3] == [0xFF, 0xD8, 0xFF] && data[data.len() - 2..] == [0xFF, 0xD9]
}

/// `POST /unggah-foto`: multipart `uuid` + `berkas` (JPEG maksimal 500 KB).
pub async fn unggah_foto(
    State(state): State<AppState>,
    PeranSopir(pengguna): PeranSopir,
    multipart: Result<Multipart, MultipartRejection>,
) -> AppResult<Response> {
    let mut multipart = multipart.map_err(|e| AppError::Validation(e.body_text()))?;
    let mut uuid: Option<Uuid> = None;
    let mut berkas: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(e.body_text()))?
    {
        match field.name() {
            Some("uuid") => {
                let teks = field
                    .text()
                    .await
                    .map_err(|e| AppError::Validation(e.body_text()))?;
                uuid = Some(
                    Uuid::parse_str(teks.trim())
                        .map_err(|_| AppError::Unprocessable("uuid bukan UUID yang sah".into()))?,
                );
            }
            Some("berkas") => {
                berkas = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| AppError::Validation(e.body_text()))?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }
    let uuid = uuid.ok_or_else(|| AppError::Unprocessable("uuid wajib diisi".into()))?;
    let berkas = berkas.ok_or_else(|| AppError::Unprocessable("berkas wajib diisi".into()))?;
    if berkas.is_empty() || berkas.len() > MAKS_UKURAN_FOTO {
        return Err(AppError::Unprocessable(format!(
            "berkas harus 1 sampai {MAKS_UKURAN_FOTO} byte (500 KB), diterima {}",
            berkas.len()
        )));
    }
    if !jpeg_valid(&berkas) {
        return Err(AppError::Unprocessable(
            "berkas harus JPEG yang utuh".into(),
        ));
    }

    let mut conn = state.pool.acquire().await?;
    let baru = sqlx::query(
        "INSERT INTO foto (id, pengunggah, ukuran, sha256, konten)
         VALUES ($1, $2, $3, encode(digest($4, 'sha256'), 'hex'), $4)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(uuid)
    .bind(&pengguna.akun)
    .bind(berkas.len() as i32)
    .bind(&berkas)
    .execute(&mut *conn)
    .await?
    .rows_affected()
        > 0;
    Ok(if baru {
        (
            StatusCode::CREATED,
            Json(json!({"foto_id": uuid, "duplikat": false})),
        )
            .into_response()
    } else {
        aksi(uuid.to_string(), true)
    })
}

/// Rute endpoint sopir (relatif terhadap `/v1`).
pub fn rute() -> Router<AppState> {
    Router::new()
        .route("/titik/{id}/tiba", post(titik_tiba))
        .route("/titik/{id}/berangkat", post(titik_berangkat))
        .route("/bukti-terima", post(bukti_terima))
        .route("/insiden", post(insiden))
        .route(
            "/unggah-foto",
            post(unggah_foto).layer(DefaultBodyLimit::max(BATAS_BADAN_UNGGAH)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_utuh() {
        assert!(jpeg_valid(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0xFF, 0xD9]));
        assert!(!jpeg_valid(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00])); // terpotong
        assert!(!jpeg_valid(b"GIF89a....")); // bukan JPEG
        assert!(!jpeg_valid(&[]));
        assert!(!jpeg_valid(&[0xFF, 0xD8, 0xFF]));
    }
}
