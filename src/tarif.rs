//! Tarif fee per kg yang dapat diubah admin (LMB-BE-008, BR-301/BR-306).
//!
//! Tarif berlaku mulai tanggal tertentu; yang dipakai untuk suatu tanggal adalah baris dengan
//! `berlaku_mulai` terbesar yang tidak melewati tanggal itu. Perubahan: pemberitahuan 30 hari,
//! maksimal 10 persen per zona terhadap tarif yang sedang berlaku, maksimal satu kali per semester.

use axum::{extract::State, http::StatusCode, response::IntoResponse, response::Response, Json};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    audit,
    auth::principal::{AdminGudangAkses, AkunAdmin},
    domain::Zona,
    error::{AppError, AppResult},
    extract::ApiJson,
    state::AppState,
    waktu,
};

/// Pemberitahuan minimal sebelum tarif baru berlaku (BR-306).
pub const PEMBERITAHUAN_HARI: i64 = 30;
/// Perubahan maksimal per zona terhadap tarif berlaku, persen (BR-306).
pub const PERUBAHAN_MAKS_PERSEN: i64 = 10;

/// Tarif dua zona, rupiah per kg.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TarifZona {
    /// Zona DEKAT.
    pub dekat: i64,
    /// Zona JAUH.
    pub jauh: i64,
}

impl TarifZona {
    /// Tarif untuk satu zona.
    pub fn untuk(self, zona: Zona) -> i64 {
        match zona {
            Zona::Dekat => self.dekat,
            Zona::Jauh => self.jauh,
        }
    }
}

/// Tarif yang berlaku pada `tanggal`.
pub async fn tarif_pada(conn: &mut PgConnection, tanggal: NaiveDate) -> AppResult<TarifZona> {
    let (dekat, jauh): (i64, i64) = sqlx::query_as(
        "SELECT dekat, jauh FROM tarif_fee WHERE berlaku_mulai <= $1 ORDER BY berlaku_mulai DESC LIMIT 1",
    )
    .bind(tanggal)
    .fetch_one(conn)
    .await?;
    Ok(TarifZona { dekat, jauh })
}

/// Semester kalender: 1 untuk Januari-Juni, 2 untuk Juli-Desember.
pub fn semester(tanggal: NaiveDate) -> (i32, u32) {
    (tanggal.year(), if tanggal.month() <= 6 { 1 } else { 2 })
}

/// Apakah `baru` berubah paling banyak 10 persen dari `lama` (aritmetika integer, tanpa pembulatan).
pub fn dalam_batas_perubahan(lama: i64, baru: i64) -> bool {
    (baru - lama).abs() * 100 <= lama * PERUBAHAN_MAKS_PERSEN
}

/// Badan `POST /admin/tarif-fee`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerbitTarifRequest {
    /// UUID klien (idempotensi).
    pub uuid: Uuid,
    /// Tanggal tarif berlaku; minimal 30 hari dari hari ini (WIB).
    pub berlaku_mulai: NaiveDate,
    /// Tarif zona DEKAT, rupiah per kg (integer lebih dari 0).
    pub dekat: i64,
    /// Tarif zona JAUH, rupiah per kg (integer lebih dari 0).
    pub jauh: i64,
}

/// `POST /admin/tarif-fee`: jadwalkan tarif baru (admin gudang atau kunci layanan), beraudit.
pub async fn terbit(
    State(state): State<AppState>,
    AdminGudangAkses(akun): AdminGudangAkses,
    ApiJson(req): ApiJson<TerbitTarifRequest>,
) -> AppResult<Response> {
    if req.dekat <= 0 || req.jauh <= 0 {
        return Err(AppError::Unprocessable(
            "tarif harus bilangan bulat rupiah lebih dari 0".into(),
        ));
    }
    let hari_ini = waktu::hari_ini_wib();
    let mut tx = state.pool.begin().await?;
    // Serialkan penulisan tarif agar aturan satu-per-semester tidak dapat dilangkahi secara bersamaan.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('tarif_fee'))")
        .execute(&mut *tx)
        .await?;

    let ada: Option<Option<String>> =
        sqlx::query_scalar("SELECT audit_id FROM tarif_fee WHERE uuid_klien = $1")
            .bind(req.uuid)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(audit_id) = ada {
        return Ok((
            StatusCode::OK,
            Json(json!({"audit_id": audit_id, "duplikat": true})),
        )
            .into_response());
    }

    if req.berlaku_mulai < hari_ini + Duration::days(PEMBERITAHUAN_HARI) {
        return Err(AppError::Unprocessable(format!(
            "tarif baru wajib diumumkan {PEMBERITAHUAN_HARI} hari sebelum berlaku (berlaku_mulai paling cepat {})",
            hari_ini + Duration::days(PEMBERITAHUAN_HARI)
        )));
    }
    let berlaku = tarif_pada(&mut tx, req.berlaku_mulai).await?;
    if !dalam_batas_perubahan(berlaku.dekat, req.dekat)
        || !dalam_batas_perubahan(berlaku.jauh, req.jauh)
    {
        return Err(AppError::Unprocessable(format!(
            "perubahan tarif maksimal {PERUBAHAN_MAKS_PERSEN} persen dari tarif berlaku (dekat {}, jauh {})",
            berlaku.dekat, berlaku.jauh
        )));
    }
    let sama: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM tarif_fee WHERE berlaku_mulai = $1)")
            .bind(req.berlaku_mulai)
            .fetch_one(&mut *tx)
            .await?;
    if sama {
        return Err(AppError::Conflict(
            "sudah ada tarif dengan berlaku_mulai yang sama".into(),
        ));
    }
    // Maksimal satu perubahan per semester (baris awal 2000-01-01 tidak dihitung).
    let (tahun, sem) = semester(req.berlaku_mulai);
    let tanggal_semester: Vec<NaiveDate> = sqlx::query_scalar(
        "SELECT berlaku_mulai FROM tarif_fee WHERE berlaku_mulai > '2000-01-01'",
    )
    .fetch_all(&mut *tx)
    .await?;
    if tanggal_semester
        .iter()
        .any(|t| semester(*t) == (tahun, sem))
    {
        return Err(AppError::Conflict(
            "tarif sudah diubah pada semester itu (maksimal satu kali per semester)".into(),
        ));
    }

    let audit_id = audit::catat_beraudit(
        &mut tx,
        &akun.aktor(),
        "tarif_fee_terbit",
        akun.perangkat_id(),
        json!({"uuid": req.uuid, "berlaku_mulai": req.berlaku_mulai, "dekat": req.dekat, "jauh": req.jauh}),
    )
    .await?;
    sqlx::query(
        "INSERT INTO tarif_fee (uuid_klien, berlaku_mulai, dekat, jauh, diterbitkan_oleh, audit_id)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(req.uuid)
    .bind(req.berlaku_mulai)
    .bind(req.dekat)
    .bind(req.jauh)
    .bind(akun.aktor())
    .bind(&audit_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"audit_id": audit_id, "duplikat": false})),
    )
        .into_response())
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct BarisTarif {
    berlaku_mulai: NaiveDate,
    dekat: i64,
    jauh: i64,
    diterbitkan_oleh: String,
    audit_id: Option<String>,
}

/// `GET /admin/tarif-fee`: tarif berlaku hari ini dan riwayat/jadwal (admin apa pun atau kunci layanan).
pub async fn daftar(
    State(state): State<AppState>,
    _akun: AkunAdmin,
) -> AppResult<Json<serde_json::Value>> {
    let mut conn = state.pool.acquire().await?;
    let sekarang = tarif_pada(&mut conn, waktu::hari_ini_wib()).await?;
    let riwayat = sqlx::query_as::<_, BarisTarif>(
        "SELECT berlaku_mulai, dekat, jauh, diterbitkan_oleh, audit_id FROM tarif_fee ORDER BY berlaku_mulai DESC",
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(json!({ "berlaku": sekarang, "riwayat": riwayat })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batas_sepuluh_persen_tepat() {
        assert!(dalam_batas_perubahan(1_500, 1_650));
        assert!(!dalam_batas_perubahan(1_500, 1_651));
        assert!(dalam_batas_perubahan(1_500, 1_350));
        assert!(!dalam_batas_perubahan(1_500, 1_349));
        assert!(dalam_batas_perubahan(2_500, 2_500));
    }

    #[test]
    fn semester_kalender() {
        let d = |m| NaiveDate::from_ymd_opt(2026, m, 15).unwrap();
        assert_eq!(semester(d(1)), (2026, 1));
        assert_eq!(semester(d(6)), (2026, 1));
        assert_eq!(semester(d(7)), (2026, 2));
        assert_eq!(semester(d(12)), (2026, 2));
    }
}
