//! Lot dan sortir (LMB-BE-004, FR-003): timbang kotor, hasil sortir A/B/C dengan toleransi 2 persen.
//!
//! Semua berat gram integer. Kode lot resmi `L-NNNN` dari server; kode sementara `TMP-{uuid8}`
//! (klien offline) diselesaikan lewat kolom `nomor_sementara` (dipetakan di LMB-BE-005).

use axum::{
    extract::Path, extract::State, http::StatusCode, response::IntoResponse, response::Response,
    Json,
};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::{
    audit,
    auth::principal::OperatorGudang,
    domain::{AksiHasil, Hasil, Komoditas, Konteks},
    error::{AppError, AppResult},
    extract::ApiJson,
    state::AppState,
    waktu,
};

/// Toleransi total sortir terhadap berat kotor: 2 persen (FSD/20 aturan sortir).
pub const TOLERANSI_PERSEN: i64 = 2;

/// Baris lot dari basis data.
#[derive(Debug, sqlx::FromRow)]
pub struct LotRow {
    /// ID internal.
    pub id: Uuid,
    /// Kode resmi `L-NNNN`.
    pub kode: String,
    /// Nomor sementara `TMP-*` yang dipetakan (bila ada).
    pub nomor_sementara: Option<String>,
    /// Kode petani.
    pub kode_petani: String,
    /// Komoditas (teks).
    pub komoditas: String,
    /// Berat kotor (gram).
    pub berat_kotor_g: i64,
    /// Grade A (gram) setelah sortir.
    pub grade_a_g: Option<i64>,
    /// Grade B (gram) setelah sortir.
    pub grade_b_g: Option<i64>,
    /// Grade C (gram) setelah sortir.
    pub grade_c_g: Option<i64>,
    /// Status lot.
    pub status: String,
    /// Versi rekaman (mulai 1).
    pub versi: i32,
    /// Pembuat (petugas).
    pub dibuat_oleh: String,
    /// Waktu dibuat.
    pub dibuat_at: DateTime<Utc>,
}

/// Kolom yang dibaca ke [`LotRow`].
pub const KOLOM_LOT: &str = "id, kode, nomor_sementara, kode_petani, komoditas, berat_kotor_g, \
     grade_a_g, grade_b_g, grade_c_g, status, versi, dibuat_oleh, dibuat_at";

/// Skema `Lot` pada kontrak.
#[derive(Debug, Serialize)]
pub struct LotJson {
    /// Kode resmi `L-NNNN`.
    pub kode: String,
    /// Kode petani.
    pub kode_petani: String,
    /// Komoditas.
    pub komoditas: String,
    /// Status lot.
    pub status: String,
    /// Berat kotor (gram).
    pub berat_kotor_g: i64,
    /// Grade A (gram), hanya setelah sortir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_a_g: Option<i64>,
    /// Grade B (gram), hanya setelah sortir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_b_g: Option<i64>,
    /// Grade C (gram), hanya setelah sortir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grade_c_g: Option<i64>,
    /// Versi rekaman untuk resolusi konflik.
    pub versi: i32,
    /// Waktu dibuat dengan offset WIB.
    pub dibuat_pada: DateTime<FixedOffset>,
    /// Petugas pembuat.
    pub dibuat_oleh: String,
    /// Selalu `false` pada respons 201.
    pub duplikat: bool,
}

impl From<LotRow> for LotJson {
    fn from(r: LotRow) -> Self {
        LotJson {
            kode: r.kode,
            kode_petani: r.kode_petani,
            komoditas: r.komoditas,
            status: r.status,
            berat_kotor_g: r.berat_kotor_g,
            grade_a_g: r.grade_a_g,
            grade_b_g: r.grade_b_g,
            grade_c_g: r.grade_c_g,
            versi: r.versi,
            dibuat_pada: r.dibuat_at.with_timezone(&waktu::wib()),
            dibuat_oleh: r.dibuat_oleh,
            duplikat: false,
        }
    }
}

/// Badan `POST /lot`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuatLotRequest {
    /// UUID klien (idempotensi).
    pub uuid: Uuid,
    /// Kode petani `PTN-NNN`.
    pub kode_petani: String,
    /// Komoditas pilot.
    pub komoditas: Komoditas,
    /// Tanggal panen/timbang.
    pub tanggal: NaiveDate,
    /// Berat kotor (gram integer, lebih dari 0).
    pub berat_kotor_g: i64,
    /// Kode petugas, contoh `OPR-SORTIR-1` (bawaan: akun sesi).
    pub petugas: Option<String>,
}

/// Badan `POST /lot/{id}/sortir`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortirRequest {
    /// UUID klien (idempotensi).
    pub uuid: Uuid,
    /// Kode lot (harus sama dengan `{id}` pada path).
    pub lot_id: String,
    /// Grade A (gram).
    pub grade_a_g: i64,
    /// Grade B (gram).
    pub grade_b_g: i64,
    /// Grade C (gram).
    pub grade_c_g: i64,
    /// Kode petugas (bawaan: akun sesi).
    pub petugas: Option<String>,
}

/// Format kode petani `PTN-NNN`.
pub fn kode_petani_valid(kode: &str) -> bool {
    kode.len() == 7 && kode.starts_with("PTN-") && kode[4..].bytes().all(|b| b.is_ascii_digit())
}

/// Total sortir sah bila selisih dengan berat kotor paling banyak 2 persen (aritmetika integer).
pub fn dalam_toleransi(kotor_g: i64, total_sortir_g: i64) -> bool {
    (total_sortir_g - kotor_g).abs() * 100 <= kotor_g * TOLERANSI_PERSEN
}

/// Cari lot lewat kode resmi atau nomor sementara; `kunci` mengunci baris (`FOR UPDATE`).
pub async fn cari_lot(
    conn: &mut PgConnection,
    kode: &str,
    kunci: bool,
) -> AppResult<Option<LotRow>> {
    let sql = format!(
        "SELECT {KOLOM_LOT} FROM lot WHERE kode = $1 OR nomor_sementara = $1{}",
        if kunci { " FOR UPDATE" } else { "" }
    );
    Ok(sqlx::query_as::<_, LotRow>(&sql)
        .bind(kode)
        .fetch_optional(conn)
        .await?)
}

fn duplikat(id: String) -> Response {
    (StatusCode::OK, Json(AksiHasil { id, duplikat: true })).into_response()
}

fn petugas_atau_akun(petugas: &Option<String>, akun: &str) -> String {
    petugas
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or(akun)
        .to_string()
}

fn validasi_lot(req: &BuatLotRequest) -> AppResult<()> {
    if !kode_petani_valid(&req.kode_petani) {
        return Err(AppError::Unprocessable("kode_petani harus PTN-NNN".into()));
    }
    if req.berat_kotor_g <= 0 {
        return Err(AppError::Unprocessable(
            "berat_kotor_g harus lebih dari 0 gram".into(),
        ));
    }
    Ok(())
}

/// Buat lot di dalam transaksi pemanggil. `nomor_sementara` (`TMP-*`) dipetakan ke kode resmi
/// pada sinkron offline. UUID klien yang sama mengembalikan [`Hasil::Duplikat`].
pub async fn buat_lot_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    req: &BuatLotRequest,
    nomor_sementara: Option<&str>,
) -> AppResult<Hasil<LotRow>> {
    validasi_lot(req)?;
    let ada: Option<String> = sqlx::query_scalar("SELECT kode FROM lot WHERE uuid_klien = $1")
        .bind(req.uuid)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(kode) = ada {
        return Ok(Hasil::Duplikat(kode));
    }
    let status_petani: Option<String> =
        sqlx::query_scalar("SELECT status FROM petani WHERE kode = $1")
            .bind(&req.kode_petani)
            .fetch_optional(&mut *conn)
            .await?;
    match status_petani.as_deref() {
        Some("AKTIF") => {}
        Some(_) => {
            return Err(AppError::Unprocessable(format!(
                "petani {} tidak aktif",
                req.kode_petani
            )))
        }
        None => {
            return Err(AppError::Unprocessable(format!(
                "petani {} tidak terdaftar",
                req.kode_petani
            )))
        }
    }

    let petugas = petugas_atau_akun(&req.petugas, &ctx.akun);
    let nomor: i64 = sqlx::query_scalar("SELECT nextval('lot_kode_seq')")
        .fetch_one(&mut *conn)
        .await?;
    let kode = format!("L-{nomor:04}");
    let sql = format!(
        "INSERT INTO lot (uuid_klien, kode, nomor_sementara, kode_petani, komoditas, tanggal, berat_kotor_g, dibuat_oleh)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {KOLOM_LOT}"
    );
    // Savepoint: pelanggaran unik tidak boleh membatalkan transaksi pemanggil.
    let mut sp = conn.begin().await?;
    let lot = match sqlx::query_as::<_, LotRow>(&sql)
        .bind(req.uuid)
        .bind(&kode)
        .bind(nomor_sementara)
        .bind(&req.kode_petani)
        .bind(req.komoditas.sebagai_str())
        .bind(req.tanggal)
        .bind(req.berat_kotor_g)
        .bind(&petugas)
        .fetch_one(&mut *sp)
        .await
    {
        Ok(l) => l,
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            drop(sp);
            // Balapan UUID yang sama, atau nomor sementara sudah dipetakan ke lot lain.
            let kode: Option<String> =
                sqlx::query_scalar("SELECT kode FROM lot WHERE uuid_klien = $1")
                    .bind(req.uuid)
                    .fetch_optional(&mut *conn)
                    .await?;
            return match kode {
                Some(k) => Ok(Hasil::Duplikat(k)),
                None => Err(AppError::Conflict(
                    "nomor sementara sudah dipakai lot lain".into(),
                )),
            };
        }
        Err(e) => return Err(e.into()),
    };
    sqlx::query(
        "INSERT INTO timbangan (lot_id, jenis, berat_g, petugas) VALUES ($1, 'KOTOR', $2, $3)",
    )
    .bind(lot.id)
    .bind(req.berat_kotor_g)
    .bind(&petugas)
    .execute(&mut *sp)
    .await?;
    sp.commit().await?;
    Ok(Hasil::Baru(lot))
}

/// `POST /lot`: buat lot dan catat timbang kotor; kode resmi `L-NNNN` dari server.
pub async fn buat(
    State(state): State<AppState>,
    OperatorGudang(gudang): OperatorGudang,
    ApiJson(req): ApiJson<BuatLotRequest>,
) -> AppResult<Response> {
    let ctx = gudang.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = buat_lot_tx(&mut tx, &ctx, &req, None).await?;
    tx.commit().await?;
    Ok(match hasil {
        Hasil::Baru(lot) => (StatusCode::CREATED, Json(LotJson::from(lot))).into_response(),
        Hasil::Duplikat(kode) => duplikat(kode),
    })
}

/// Catat hasil sortir di dalam transaksi pemanggil. `id` adalah kode dari path (atau `lot_id`
/// rekaman sinkron) dan harus sama dengan `req.lot_id`.
pub async fn sortir_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    id: &str,
    req: &SortirRequest,
) -> AppResult<Hasil<LotRow>> {
    if [req.grade_a_g, req.grade_b_g, req.grade_c_g]
        .iter()
        .any(|g| *g < 0)
    {
        return Err(AppError::Unprocessable(
            "berat grade tidak boleh negatif".into(),
        ));
    }
    // Idempotensi dahulu: kirim ulang tidak boleh gagal hanya karena lot sudah berubah status.
    let ada: Option<String> = sqlx::query_scalar(
        "SELECT l.kode FROM sortir s JOIN lot l ON l.id = s.lot_id WHERE s.uuid_klien = $1",
    )
    .bind(req.uuid)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(kode) = ada {
        return Ok(Hasil::Duplikat(kode));
    }

    let lot = cari_lot(conn, id, true).await?.ok_or(AppError::NotFound)?;
    let cocok = |k: &str| k == lot.kode || Some(k) == lot.nomor_sementara.as_deref();
    if !cocok(&req.lot_id) {
        return Err(AppError::Unprocessable(
            "lot_id pada badan berbeda dengan {id} pada path".into(),
        ));
    }
    if lot.status != "BARU" {
        return Err(AppError::Conflict(
            "konflik versi, data gudang lebih baru".into(),
        ));
    }
    let total = req.grade_a_g + req.grade_b_g + req.grade_c_g;
    if !dalam_toleransi(lot.berat_kotor_g, total) {
        return Err(AppError::Unprocessable(
            "total sortir menyimpang lebih dari 2 persen dari berat kotor".into(),
        ));
    }

    let petugas = petugas_atau_akun(&req.petugas, &ctx.akun);
    sqlx::query(
        "INSERT INTO sortir (uuid_klien, lot_id, grade_a_g, grade_b_g, grade_c_g, petugas)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(req.uuid)
    .bind(lot.id)
    .bind(req.grade_a_g)
    .bind(req.grade_b_g)
    .bind(req.grade_c_g)
    .bind(&petugas)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO timbangan (lot_id, jenis, berat_g, petugas) VALUES ($1, 'BERSIH', $2, $3)",
    )
    .bind(lot.id)
    .bind(total)
    .bind(&petugas)
    .execute(&mut *conn)
    .await?;
    if req.grade_c_g > 0 {
        // FR-003: Grade C tidak masuk rantai dingin; alihnya dicatat.
        sqlx::query("INSERT INTO alih_grade_c (lot_id, berat_g) VALUES ($1, $2)")
            .bind(lot.id)
            .bind(req.grade_c_g)
            .execute(&mut *conn)
            .await?;
        audit::catat(
            conn,
            &ctx.akun,
            "grade_c_dialihkan",
            ctx.perangkat.as_deref(),
            json!({"lot": lot.kode, "berat_g": req.grade_c_g}),
        )
        .await?;
    }
    let sql = format!(
        "UPDATE lot SET grade_a_g = $2, grade_b_g = $3, grade_c_g = $4, status = 'DISORTIR',
                versi = versi + 1, diperbarui_at = now()
         WHERE id = $1 RETURNING {KOLOM_LOT}"
    );
    let baru = sqlx::query_as::<_, LotRow>(&sql)
        .bind(lot.id)
        .bind(req.grade_a_g)
        .bind(req.grade_b_g)
        .bind(req.grade_c_g)
        .fetch_one(&mut *conn)
        .await?;
    Ok(Hasil::Baru(baru))
}

/// `POST /lot/{id}/sortir`: catat hasil sortir A/B/C; total harus dalam 2 persen berat kotor.
pub async fn sortir(
    State(state): State<AppState>,
    OperatorGudang(gudang): OperatorGudang,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<SortirRequest>,
) -> AppResult<Response> {
    let ctx = gudang.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = sortir_tx(&mut tx, &ctx, &id, &req).await?;
    tx.commit().await?;
    Ok(match hasil {
        Hasil::Baru(lot) => (StatusCode::CREATED, Json(LotJson::from(lot))).into_response(),
        Hasil::Duplikat(kode) => duplikat(kode),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toleransi_dua_persen_tepat_di_batas() {
        // Contoh L-8812: 100.000 + 14.000 + 6.000 = 120.000 tepat.
        assert!(dalam_toleransi(120_000, 120_000));
        // Batas atas dan bawah: 2 persen dari 120.000 = 2.400 gram.
        assert!(dalam_toleransi(120_000, 122_400));
        assert!(!dalam_toleransi(120_000, 122_401));
        assert!(dalam_toleransi(120_000, 117_600));
        assert!(!dalam_toleransi(120_000, 117_599));
        // Tanpa pembulatan desimal: 2 persen dari 1.001 = 20,02 gram.
        assert!(dalam_toleransi(1_001, 1_021));
        assert!(!dalam_toleransi(1_001, 1_022));
    }

    #[test]
    fn format_kode_petani() {
        assert!(kode_petani_valid("PTN-007"));
        assert!(!kode_petani_valid("PTN-7"));
        assert!(!kode_petani_valid("PTN-0071"));
        assert!(!kode_petani_valid("ptn-007"));
        assert!(!kode_petani_valid("PTN-0a7"));
        assert!(!kode_petani_valid("PTN-００７"));
    }
}
