//! Keranjang batch dan stok-lot (LMB-BE-004).
//!
//! Keranjang berkode permanen `KRJ-001`..`KRJ-120` (keranjang fisik dipakai berulang): satu kode
//! hanya boleh punya satu isi aktif. Grade C ditolak dari rantai dingin (FR-003). Stok per lot dan
//! grade = hasil sortir dikurangi isi keranjang; pengecekan memakai kunci baris lot agar dua batch
//! bersamaan tidak sama-sama lolos melewati stok.

use std::collections::{BTreeMap, HashSet};

use axum::{extract::State, http::StatusCode, response::IntoResponse, response::Response, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::{
    audit,
    auth::{peran::Peran, principal::Gudang},
    domain::{AksiHasil, Grade, Hasil, Konteks},
    error::{AppError, AppResult},
    extract::ApiJson,
    lot::cari_lot,
    state::AppState,
};

/// Maksimal keranjang per request `POST /keranjang/batch`.
pub const MAKS_KERANJANG_PER_BATCH: usize = 50;
/// Nomor keranjang fisik tertinggi (`KRJ-120`).
pub const MAKS_NOMOR_KERANJANG: u32 = 120;

/// Satu keranjang pada `KeranjangBatchRequest`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeranjangItem {
    /// Kode keranjang fisik `KRJ-NNN`.
    pub kode: String,
    /// Kode lot (`L-NNNN` atau `TMP-*`).
    pub lot_id: String,
    /// Grade isi keranjang.
    pub grade: Grade,
    /// Berat isi (gram, lebih dari 0).
    pub berat_g: i64,
}

/// Badan `POST /keranjang/batch`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeranjangBatchRequest {
    /// UUID klien untuk seluruh batch (idempotensi).
    pub uuid: Uuid,
    /// 1 sampai 50 keranjang.
    pub keranjang: Vec<KeranjangItem>,
}

/// Skema `StokLot`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct StokLot {
    /// Kode lot.
    pub lot_id: String,
    /// Komoditas.
    pub komoditas: String,
    /// Grade (A atau B; C tidak masuk rantai dingin).
    pub grade: String,
    /// Sisa gram yang belum masuk keranjang.
    pub sisa_g: i64,
}

/// Kode keranjang sah: `KRJ-NNN` dengan nomor 001..120.
pub fn kode_keranjang_valid(kode: &str) -> bool {
    kode.len() == 7
        && kode.starts_with("KRJ-")
        && kode[4..].bytes().all(|b| b.is_ascii_digit())
        && matches!(kode[4..].parse::<u32>(), Ok(n) if (1..=MAKS_NOMOR_KERANJANG).contains(&n))
}

fn validasi(req: &KeranjangBatchRequest) -> AppResult<()> {
    let n = req.keranjang.len();
    if n == 0 {
        return Err(AppError::Unprocessable(
            "keranjang minimal 1 per request".into(),
        ));
    }
    if n > MAKS_KERANJANG_PER_BATCH {
        return Err(AppError::Unprocessable(format!(
            "keranjang maksimal {MAKS_KERANJANG_PER_BATCH} per request, diterima {n}"
        )));
    }
    let mut dilihat = HashSet::new();
    for item in &req.keranjang {
        if !kode_keranjang_valid(&item.kode) {
            return Err(AppError::Unprocessable(format!(
                "kode keranjang {} tidak sah (KRJ-001 sampai KRJ-{MAKS_NOMOR_KERANJANG})",
                item.kode
            )));
        }
        if !dilihat.insert(item.kode.as_str()) {
            return Err(AppError::Unprocessable(format!(
                "kode keranjang {} dobel dalam satu batch",
                item.kode
            )));
        }
        if item.berat_g <= 0 {
            return Err(AppError::Unprocessable(format!(
                "berat keranjang {} harus lebih dari 0 gram",
                item.kode
            )));
        }
    }
    Ok(())
}

/// Tolak Grade C dari rantai dingin dan catat percobaannya di audit (FR-003). Dipanggil di luar
/// transaksi batch agar catatan tetap tersimpan walau batch dibatalkan.
pub async fn periksa_grade_c(
    pool: &sqlx::PgPool,
    ctx: &Konteks,
    req: &KeranjangBatchRequest,
) -> AppResult<()> {
    if let Some(c) = req.keranjang.iter().find(|k| k.grade == Grade::C) {
        let mut conn = pool.acquire().await?;
        audit::catat(
            &mut conn,
            &ctx.akun,
            "grade_c_ditolak_rantai_dingin",
            ctx.perangkat.as_deref(),
            json!({"keranjang": c.kode, "lot": c.lot_id, "berat_g": c.berat_g}),
        )
        .await?;
        return Err(AppError::Unprocessable(
            "Grade C ditolak dari rantai dingin; alihnya dicatat".into(),
        ));
    }
    Ok(())
}

/// Catat batch keranjang di dalam transaksi pemanggil (atomik: gagal sebagian = tidak ada yang
/// tersimpan bila pemanggil membatalkan transaksinya).
pub async fn batch_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    req: &KeranjangBatchRequest,
) -> AppResult<Hasil<()>> {
    validasi(req)?;
    if req.keranjang.iter().any(|k| k.grade == Grade::C) {
        return Err(AppError::Unprocessable(
            "Grade C ditolak dari rantai dingin; alihnya dicatat".into(),
        ));
    }
    let ada: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM keranjang_batch WHERE uuid_klien = $1)")
            .bind(req.uuid)
            .fetch_one(&mut *conn)
            .await?;
    if ada {
        return Ok(Hasil::Duplikat(req.uuid.to_string()));
    }

    // Kunci lot berurutan (BTreeMap) agar dua batch bersamaan tidak saling kunci-mati.
    let mut per_lot: BTreeMap<&str, Vec<&KeranjangItem>> = BTreeMap::new();
    for item in &req.keranjang {
        per_lot.entry(item.lot_id.as_str()).or_default().push(item);
    }
    let mut id_lot: BTreeMap<&str, Uuid> = BTreeMap::new();
    for (kode, items) in &per_lot {
        let lot = cari_lot(conn, kode, true)
            .await?
            .ok_or(AppError::NotFound)?;
        if !matches!(lot.status.as_str(), "DISORTIR" | "DIMUAT") {
            return Err(AppError::Unprocessable(format!(
                "lot {} berstatus {}; hanya lot yang sudah disortir yang bisa dikemas",
                lot.kode, lot.status
            )));
        }
        for grade in [Grade::A, Grade::B] {
            let diminta: i64 = items
                .iter()
                .filter(|i| i.grade == grade)
                .map(|i| i.berat_g)
                .sum();
            if diminta == 0 {
                continue;
            }
            let sisa = sisa_grade(conn, lot.id, grade, &lot).await?;
            if diminta > sisa {
                return Err(AppError::Unprocessable(format!(
                    "stok lot {} grade {} tidak cukup: diminta {diminta} g, sisa {sisa} g",
                    lot.kode,
                    grade.sebagai_str()
                )));
            }
        }
        id_lot.insert(kode, lot.id);
    }

    sqlx::query(
        "INSERT INTO keranjang_batch (uuid_klien, jumlah, dibuat_oleh) VALUES ($1, $2, $3)",
    )
    .bind(req.uuid)
    .bind(req.keranjang.len() as i32)
    .bind(&ctx.akun)
    .execute(&mut *conn)
    .await?;
    for item in &req.keranjang {
        let lot_id = id_lot[item.lot_id.as_str()];
        // Savepoint: pelanggaran unik (keranjang masih terisi) tidak boleh merusak transaksi pemanggil.
        let mut sp = conn.begin().await?;
        let hasil = sqlx::query(
            "INSERT INTO keranjang_isi (batch_uuid, kode, lot_id, grade, berat_g)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(req.uuid)
        .bind(&item.kode)
        .bind(lot_id)
        .bind(item.grade.sebagai_str())
        .bind(item.berat_g)
        .execute(&mut *sp)
        .await;
        match hasil {
            Ok(_) => sp.commit().await?,
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(AppError::Conflict(format!(
                    "keranjang {} masih terisi dan belum dikosongkan",
                    item.kode
                )));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(Hasil::Baru(()))
}

/// `POST /keranjang/batch`: catat hingga 50 keranjang secara atomik (semua atau tidak sama sekali).
pub async fn batch(
    State(state): State<AppState>,
    gudang: Gudang,
    ApiJson(req): ApiJson<KeranjangBatchRequest>,
) -> AppResult<Response> {
    gudang
        .pengguna
        .wajib_peran(&state, &[Peran::AdminGudang])
        .await?;
    validasi(&req)?;
    let ctx = gudang.konteks();
    periksa_grade_c(&state.pool, &ctx, &req).await?;

    let mut tx = state.pool.begin().await?;
    let hasil = batch_tx(&mut tx, &ctx, &req).await?;
    tx.commit().await?;
    let (status, duplikat) = match hasil {
        Hasil::Baru(()) => (StatusCode::CREATED, false),
        Hasil::Duplikat(_) => (StatusCode::OK, true),
    };
    Ok((
        status,
        Json(AksiHasil {
            id: req.uuid.to_string(),
            duplikat,
        }),
    )
        .into_response())
}

async fn sisa_grade(
    conn: &mut PgConnection,
    lot_id: Uuid,
    grade: Grade,
    lot: &crate::lot::LotRow,
) -> AppResult<i64> {
    let total = match grade {
        Grade::A => lot.grade_a_g,
        Grade::B => lot.grade_b_g,
        Grade::C => None,
    }
    .unwrap_or(0);
    let terpakai: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(berat_g), 0)::bigint FROM keranjang_isi WHERE lot_id = $1 AND grade = $2",
    )
    .bind(lot_id)
    .bind(grade.sebagai_str())
    .fetch_one(conn)
    .await?;
    Ok(total - terpakai)
}

/// `GET /stok-lot`: sisa per lot dan grade (A/B) yang masih bisa dialokasikan.
pub async fn stok_lot(
    State(state): State<AppState>,
    gudang: Gudang,
) -> AppResult<Json<serde_json::Value>> {
    gudang
        .pengguna
        .wajib_peran(&state, &[Peran::AdminGudang])
        .await?;
    let data = sqlx::query_as::<_, StokLot>(
        "SELECT kode AS lot_id, komoditas, grade, sisa_g FROM (
            SELECT l.kode, l.komoditas, 'A' AS grade,
                   l.grade_a_g - COALESCE((SELECT SUM(k.berat_g) FROM keranjang_isi k
                                           WHERE k.lot_id = l.id AND k.grade = 'A'), 0)::bigint AS sisa_g
              FROM lot l WHERE l.status IN ('DISORTIR', 'DIMUAT')
            UNION ALL
            SELECT l.kode, l.komoditas, 'B',
                   l.grade_b_g - COALESCE((SELECT SUM(k.berat_g) FROM keranjang_isi k
                                           WHERE k.lot_id = l.id AND k.grade = 'B'), 0)::bigint
              FROM lot l WHERE l.status IN ('DISORTIR', 'DIMUAT')
         ) s WHERE sisa_g > 0 ORDER BY kode, grade",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({ "data": data })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kode_keranjang_001_sampai_120() {
        assert!(kode_keranjang_valid("KRJ-001"));
        assert!(kode_keranjang_valid("KRJ-120"));
        assert!(!kode_keranjang_valid("KRJ-000"));
        assert!(!kode_keranjang_valid("KRJ-121"));
        assert!(!kode_keranjang_valid("KRJ-1"));
        assert!(!kode_keranjang_valid("krj-001"));
        assert!(!kode_keranjang_valid("KRJ-0a1"));
    }
}
