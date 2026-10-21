//! Pembayaran petani H+1 (LMB-BE-006, BR-104/FR-204).
//!
//! Dibuat saat sortir dicatat (berat bersih per grade diketahui): jatuh tempo H+1 (tanggal WIB).
//! Nominal = berat bersih Grade A x harga papan A + berat bersih Grade B x harga papan B, dengan
//! papan minggu berjalan pada tanggal sortir (harga B papan sudah memuat faktor 85 persen, ADR-0005).
//! Grade C tidak dibayar (dialihkan). Denda Lumbung 0,5 persen per hari keterlambatan, maksimal 5 persen.

use std::collections::HashMap;

use axum::{extract::State, Json};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::{PgConnection, PgPool};

use crate::{
    auth::principal::PeranPetani,
    domain::Komoditas,
    error::{AppError, AppResult},
    fee::bagi_bulat,
    lot::LotRow,
    papan_harga::{papan_berlaku, HargaGrade, PapanHarga},
    state::AppState,
    waktu,
};

/// Denda per hari keterlambatan, dalam per mil (0,5 persen = 5 per mil).
pub const DENDA_PER_MIL_PER_HARI: i64 = 5;
/// Denda maksimal, dalam per mil (5 persen).
pub const DENDA_MAKS_PER_MIL: i64 = 50;

/// Nominal pembayaran dari berat bersih (gram) dan harga papan per kg (rupiah).
pub fn hitung_nominal(a_g: i64, b_g: i64, harga: HargaGrade) -> i64 {
    bagi_bulat(a_g * harga.a, 1000) + bagi_bulat(b_g * harga.b, 1000)
}

/// Denda Lumbung: 0,5 persen per hari lewat jatuh tempo, maksimal 5 persen dari nominal.
pub fn denda_pembayaran(nominal: i64, jatuh_tempo: NaiveDate, hari_ini: NaiveDate) -> i64 {
    let terlambat = (hari_ini - jatuh_tempo).num_days();
    if terlambat <= 0 {
        return 0;
    }
    let per_mil = (terlambat * DENDA_PER_MIL_PER_HARI).min(DENDA_MAKS_PER_MIL);
    bagi_bulat(nominal * per_mil, 1000)
}

/// Harga papan per grade untuk satu komoditas.
fn harga_komoditas(papan: &PapanHarga, komoditas: Komoditas) -> HargaGrade {
    match komoditas {
        Komoditas::Cabai => papan.cabai,
        Komoditas::Bawang => papan.bawang,
        Komoditas::Kangkung => papan.kangkung,
    }
}

/// Catat kewajiban bayar petani saat sortir selesai (dipanggil dari `sortir_tx`).
pub async fn catat_sortir(conn: &mut PgConnection, lot: &LotRow) -> AppResult<()> {
    let (a, b) = (lot.grade_a_g.unwrap_or(0), lot.grade_b_g.unwrap_or(0));
    let tanggal = waktu::hari_ini_wib();
    sqlx::query(
        "INSERT INTO pembayaran_petani (lot_id, kode_petani, berat_a_g, berat_b_g, tanggal, jatuh_tempo)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (lot_id) DO NOTHING",
    )
    .bind(lot.id)
    .bind(&lot.kode_petani)
    .bind(a)
    .bind(b)
    .bind(tanggal)
    .bind(tanggal + Duration::days(1))
    .execute(conn)
    .await?;
    Ok(())
}

/// Skema `Pembayaran` pada kontrak.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PembayaranJson {
    /// Kode lot.
    pub lot_id: String,
    /// Nominal yang dibayarkan ke petani (rupiah).
    pub nominal: i64,
    /// `MENUNGGU`, `DIBAYAR`, atau `TERLAMBAT`.
    pub status: &'static str,
    /// Jatuh tempo H+1 (tanggal WIB).
    pub jatuh_tempo: NaiveDate,
    /// Denda Lumbung (rupiah): berjalan bila belum dibayar, beku bila sudah.
    pub denda: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct BarisBayar {
    lot_kode: String,
    komoditas: String,
    berat_a_g: i64,
    berat_b_g: i64,
    tanggal: NaiveDate,
    jatuh_tempo: NaiveDate,
    dibayar_pada: Option<DateTime<Utc>>,
    nominal_dibayar: Option<i64>,
    denda_dibayar: Option<i64>,
}

const SQL_BARIS: &str =
    "SELECT l.kode AS lot_kode, l.komoditas, p.berat_a_g, p.berat_b_g, p.tanggal,
        p.jatuh_tempo, p.dibayar_pada, p.nominal_dibayar, p.denda_dibayar
     FROM pembayaran_petani p JOIN lot l ON l.id = p.lot_id";

/// Susun status/nominal/denda satu baris; `None` bila papan minggu itu belum terbit (belum bisa dihitung).
fn susun(
    b: &BarisBayar,
    papan: &HashMap<NaiveDate, Option<PapanHarga>>,
    hari_ini: NaiveDate,
) -> Option<PembayaranJson> {
    if b.dibayar_pada.is_some() {
        return Some(PembayaranJson {
            lot_id: b.lot_kode.clone(),
            nominal: b.nominal_dibayar.unwrap_or(0),
            status: "DIBAYAR",
            jatuh_tempo: b.jatuh_tempo,
            denda: b.denda_dibayar.unwrap_or(0),
        });
    }
    let komoditas = Komoditas::dari_str(&b.komoditas)?;
    let papan = papan.get(&b.tanggal)?.as_ref()?;
    let nominal = hitung_nominal(b.berat_a_g, b.berat_b_g, harga_komoditas(papan, komoditas));
    let terlambat = hari_ini > b.jatuh_tempo;
    Some(PembayaranJson {
        lot_id: b.lot_kode.clone(),
        nominal,
        status: if terlambat { "TERLAMBAT" } else { "MENUNGGU" },
        jatuh_tempo: b.jatuh_tempo,
        denda: denda_pembayaran(nominal, b.jatuh_tempo, hari_ini),
    })
}

async fn papan_per_tanggal(
    pool: &PgPool,
    baris: &[BarisBayar],
) -> AppResult<HashMap<NaiveDate, Option<PapanHarga>>> {
    let mut peta = HashMap::new();
    for b in baris {
        if b.dibayar_pada.is_none() && !peta.contains_key(&b.tanggal) {
            peta.insert(b.tanggal, papan_berlaku(pool, b.tanggal).await?);
        }
    }
    Ok(peta)
}

/// Pembayaran milik satu petani (difilter server per kode petani), terbaru lebih dulu.
/// Lot yang papan minggunya belum terbit belum tampil karena nominalnya belum bisa dihitung.
pub async fn daftar_petani(
    pool: &PgPool,
    kode_petani: &str,
    hari_ini: NaiveDate,
) -> AppResult<Vec<PembayaranJson>> {
    let sql = format!("{SQL_BARIS} WHERE p.kode_petani = $1 ORDER BY p.tanggal DESC, l.kode DESC");
    let baris = sqlx::query_as::<_, BarisBayar>(&sql)
        .bind(kode_petani)
        .fetch_all(pool)
        .await?;
    let papan = papan_per_tanggal(pool, &baris).await?;
    Ok(baris
        .iter()
        .filter_map(|b| susun(b, &papan, hari_ini))
        .collect())
}

/// Tandai lot sudah dibayar: nominal dan denda dibekukan pada nilai tanggal `hari_ini`.
/// Dipakai alur keuangan (belum ada endpoint HTTP di kontrak v1.0.0, lihat ADR-0005).
pub async fn tandai_dibayar(
    pool: &PgPool,
    lot_kode: &str,
    hari_ini: NaiveDate,
) -> AppResult<PembayaranJson> {
    let mut tx = pool.begin().await?;
    let sql = format!("{SQL_BARIS} WHERE l.kode = $1 FOR UPDATE OF p");
    let baris = sqlx::query_as::<_, BarisBayar>(&sql)
        .bind(lot_kode)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if baris.dibayar_pada.is_some() {
        return Err(AppError::Conflict("pembayaran sudah dibayar".into()));
    }
    let papan = papan_per_tanggal(pool, std::slice::from_ref(&baris)).await?;
    let hasil = susun(&baris, &papan, hari_ini).ok_or_else(|| {
        AppError::Unprocessable(
            "papan harga minggu sortir belum terbit; nominal belum bisa dihitung".into(),
        )
    })?;
    sqlx::query(
        "UPDATE pembayaran_petani SET dibayar_pada = now(), nominal_dibayar = $2, denda_dibayar = $3
         WHERE lot_id = (SELECT id FROM lot WHERE kode = $1)",
    )
    .bind(lot_kode)
    .bind(hasil.nominal)
    .bind(hasil.denda)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PembayaranJson {
        status: "DIBAYAR",
        ..hasil
    })
}

/// `GET /pembayaran-saya`: pembayaran petani yang sedang login, difilter server per id petani.
pub async fn saya(
    State(state): State<AppState>,
    PeranPetani(petani): PeranPetani,
) -> AppResult<Json<serde_json::Value>> {
    // Akun peran petani sama dengan kode petani (`PTN-NNN`); tidak pernah dibaca dari parameter klien.
    let data = daftar_petani(&state.pool, &petani.akun, waktu::hari_ini_wib()).await?;
    Ok(Json(json!({ "data": data })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, dd: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, dd).unwrap()
    }

    #[test]
    fn nominal_berat_bersih_per_grade_kali_harga_papan() {
        // Contoh L-8812: A 100.000 g x 31.500 + B 14.000 g x 26.500 = 3.150.000 + 371.000.
        let harga = HargaGrade {
            a: 31_500,
            b: 26_500,
        };
        assert_eq!(hitung_nominal(100_000, 14_000, harga), 3_521_000);
        assert_eq!(hitung_nominal(0, 0, harga), 0);
        // Pecahan kg dibulatkan setengah naik: 333 g x 31.500 / 1.000 = 10.489,5 -> 10.490.
        assert_eq!(hitung_nominal(333, 0, harga), 10_490);
    }

    #[test]
    fn denda_setengah_persen_per_hari_maks_lima_persen() {
        let jt = d(2026, 10, 12);
        let n = 3_521_000;
        assert_eq!(
            denda_pembayaran(n, jt, d(2026, 10, 12)),
            0,
            "tepat jatuh tempo"
        );
        assert_eq!(denda_pembayaran(n, jt, d(2026, 10, 11)), 0);
        assert_eq!(
            denda_pembayaran(n, jt, d(2026, 10, 13)),
            17_605,
            "1 hari = 0,5 persen"
        );
        assert_eq!(
            denda_pembayaran(n, jt, d(2026, 10, 15)),
            52_815,
            "3 hari = 1,5 persen"
        );
        assert_eq!(
            denda_pembayaran(n, jt, d(2026, 10, 22)),
            176_050,
            "10 hari = 5 persen"
        );
        assert_eq!(
            denda_pembayaran(n, jt, d(2026, 12, 31)),
            176_050,
            "dibatasi 5 persen"
        );
    }
}
