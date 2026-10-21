//! Invoice fee mingguan per pembeli (LMB-BE-006, BR-302).
//!
//! Invoice terbit tiap Senin untuk surat jalan minggu ISO sebelumnya, jatuh tempo 7 hari setelah
//! terbit, denda 1 persen per minggu keterlambatan (minggu yang sedang berjalan dihitung penuh),
//! maksimal 4 persen. Generasi idempoten dan tidak butuh penjadwal: dijalankan malas saat daftar
//! invoice dibaca dan oleh tugas latar `terbitkan_tertunda` (main.rs).

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::Serialize;
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    audit,
    auth::principal::AdminKeuanganAkses,
    error::{AppError, AppResult},
    fee::bagi_bulat,
    state::AppState,
    urutan, waktu,
};

/// Jatuh tempo: 7 hari setelah terbit (BR-302).
pub const JATUH_TEMPO_HARI: i64 = 7;
/// Denda 1 persen per minggu keterlambatan (BR-302).
pub const DENDA_PERSEN_PER_MINGGU: i64 = 1;
/// Denda maksimal 4 persen (BR-302).
pub const DENDA_MAKS_PERSEN: i64 = 4;

/// Senin pada minggu ISO yang memuat `tanggal`.
pub fn senin_dari(tanggal: NaiveDate) -> NaiveDate {
    tanggal - Duration::days(tanggal.weekday().num_days_from_monday() as i64)
}

/// Format minggu ISO `YYYY-Www` dari hari Senin awal minggu.
pub fn format_minggu(senin: NaiveDate) -> String {
    let w = senin.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

/// Urai `YYYY-Www` menjadi hari Senin minggu itu; `None` bila format atau nomor minggu salah.
pub fn urai_minggu(teks: &str) -> Option<NaiveDate> {
    let b = teks.as_bytes();
    if b.len() != 8 || b[4] != b'-' || b[5] != b'W' {
        return None;
    }
    if !b[..4].iter().chain(&b[6..]).all(u8::is_ascii_digit) {
        return None;
    }
    let tahun: i32 = teks[..4].parse().ok()?;
    let minggu: u32 = teks[6..].parse().ok()?;
    NaiveDate::from_isoywd_opt(tahun, minggu, Weekday::Mon)
}

/// Denda invoice: 1 persen per minggu (atau bagian minggu) lewat jatuh tempo, maksimal 4 persen.
pub fn denda_invoice(nominal: i64, jatuh_tempo: NaiveDate, hari_ini: NaiveDate) -> i64 {
    let terlambat = (hari_ini - jatuh_tempo).num_days();
    if terlambat <= 0 {
        return 0;
    }
    let minggu = (terlambat + 6) / 7;
    let persen = (minggu * DENDA_PERSEN_PER_MINGGU).min(DENDA_MAKS_PERSEN);
    bagi_bulat(nominal * persen, 100)
}

/// Skema `Invoice` pada kontrak.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct InvoiceJson {
    /// Kode invoice `INV-YYYYMMDD-NNN` (tanggal terbit).
    pub id: String,
    /// Pembeli.
    pub pembeli: String,
    /// Minggu ISO surat jalan yang ditagih.
    pub minggu: String,
    /// Total fee (rupiah), tanpa denda.
    pub nominal: i64,
    /// `TERBIT`, `LUNAS`, atau `LEWAT_JATUH_TEMPO`.
    pub status: &'static str,
    /// Tanggal jatuh tempo.
    pub jatuh_tempo: NaiveDate,
    /// Denda (rupiah): berjalan bila belum lunas, beku bila lunas.
    pub denda: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct InvoiceRow {
    id: Uuid,
    kode: String,
    pembeli: String,
    minggu: String,
    nominal: i64,
    jatuh_tempo: NaiveDate,
    dilunasi_at: Option<chrono::DateTime<chrono::Utc>>,
    denda: Option<i64>,
}

const KOLOM: &str = "id, kode, pembeli, minggu, nominal, jatuh_tempo, dilunasi_at, denda";

fn ke_json(r: InvoiceRow, hari_ini: NaiveDate) -> InvoiceJson {
    let (status, denda) = if r.dilunasi_at.is_some() {
        ("LUNAS", r.denda.unwrap_or(0))
    } else if hari_ini > r.jatuh_tempo {
        (
            "LEWAT_JATUH_TEMPO",
            denda_invoice(r.nominal, r.jatuh_tempo, hari_ini),
        )
    } else {
        ("TERBIT", 0)
    };
    InvoiceJson {
        id: r.kode,
        pembeli: r.pembeli,
        minggu: r.minggu,
        nominal: r.nominal,
        status,
        jatuh_tempo: r.jatuh_tempo,
        denda,
    }
}

/// Terbitkan invoice untuk minggu ISO yang dimulai `senin` bila minggu itu sudah berakhir.
/// Idempoten: fee yang sudah masuk invoice dilewati. Mengembalikan jumlah invoice baru.
pub async fn terbitkan_invoice_minggu(
    conn: &mut PgConnection,
    senin: NaiveDate,
    hari_ini: NaiveDate,
) -> AppResult<usize> {
    let terbit = senin + Duration::days(7);
    if hari_ini < terbit {
        return Ok(0); // minggu belum berakhir; terbit tiap Senin berikutnya
    }
    let minggu = format_minggu(senin);
    // Serialkan generasi untuk minggu yang sama agar penomoran dan lampiran fee tidak berbalapan.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("invoice:{minggu}"))
        .execute(&mut *conn)
        .await?;
    let per_pembeli: Vec<(String, i64)> = sqlx::query_as(
        "SELECT pembeli, SUM(nominal)::bigint FROM fee
         WHERE invoice_id IS NULL AND pembeli IS NOT NULL AND tanggal BETWEEN $1 AND $2
         GROUP BY pembeli ORDER BY pembeli",
    )
    .bind(senin)
    .bind(senin + Duration::days(6))
    .fetch_all(&mut *conn)
    .await?;
    let mut baru = 0;
    for (pembeli, nominal) in per_pembeli {
        let urut = urutan::berikutnya(&mut *conn, "INV", terbit).await?;
        let kode = urutan::format_nomor("INV", terbit, urut);
        let id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO invoice (kode, pembeli, minggu, minggu_mulai, terbit, jatuh_tempo, nominal)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (pembeli, minggu) DO NOTHING RETURNING id",
        )
        .bind(&kode)
        .bind(&pembeli)
        .bind(&minggu)
        .bind(senin)
        .bind(terbit)
        .bind(terbit + Duration::days(JATUH_TEMPO_HARI))
        .bind(nominal)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(id) = id {
            sqlx::query(
                "UPDATE fee SET invoice_id = $1
                 WHERE invoice_id IS NULL AND pembeli = $2 AND tanggal BETWEEN $3 AND $4",
            )
            .bind(id)
            .bind(&pembeli)
            .bind(senin)
            .bind(senin + Duration::days(6))
            .execute(&mut *conn)
            .await?;
            baru += 1;
        }
    }
    Ok(baru)
}

/// Terbitkan semua invoice tertunda untuk minggu yang sudah berakhir. Dipanggil berkala dari
/// tugas latar; aman diulang. Mengembalikan jumlah invoice baru.
pub async fn terbitkan_tertunda(pool: &PgPool, hari_ini: NaiveDate) -> AppResult<usize> {
    let batas = senin_dari(hari_ini);
    let minggu: Vec<NaiveDate> = sqlx::query_scalar(
        "SELECT DISTINCT date_trunc('week', tanggal::timestamp)::date FROM fee
         WHERE invoice_id IS NULL AND pembeli IS NOT NULL AND tanggal < $1 ORDER BY 1",
    )
    .bind(batas)
    .fetch_all(pool)
    .await?;
    let mut total = 0;
    for senin in minggu {
        let mut tx = pool.begin().await?;
        total += terbitkan_invoice_minggu(&mut tx, senin, hari_ini).await?;
        tx.commit().await?;
    }
    Ok(total)
}

/// `GET /admin/invoice?minggu=YYYY-Www`: invoice fee mingguan (admin keuangan/gudang atau kunci layanan).
pub async fn daftar(
    State(state): State<AppState>,
    _akun: AdminKeuanganAkses,
    Query(parameter): Query<HashMap<String, String>>,
) -> AppResult<Json<serde_json::Value>> {
    let teks = parameter
        .get("minggu")
        .ok_or_else(|| AppError::Unprocessable("parameter minggu wajib (YYYY-Www)".into()))?;
    let senin = urai_minggu(teks).ok_or_else(|| {
        AppError::Unprocessable(
            "minggu harus berformat YYYY-Www dengan nomor minggu ISO sah".into(),
        )
    })?;
    let hari_ini = waktu::hari_ini_wib();
    let mut tx = state.pool.begin().await?;
    terbitkan_invoice_minggu(&mut tx, senin, hari_ini).await?;
    let sql = format!("SELECT {KOLOM} FROM invoice WHERE minggu = $1 ORDER BY kode");
    let baris = sqlx::query_as::<_, InvoiceRow>(&sql)
        .bind(format_minggu(senin))
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let data: Vec<InvoiceJson> = baris.into_iter().map(|r| ke_json(r, hari_ini)).collect();
    Ok(Json(json!({ "data": data })))
}

/// `POST /admin/invoice/{id}/lunas`: tandai lunas, bekukan denda, beraudit (`audit_id`).
pub async fn lunas(
    State(state): State<AppState>,
    AdminKeuanganAkses(akun): AdminKeuanganAkses,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let hari_ini = waktu::hari_ini_wib();
    let mut tx = state.pool.begin().await?;
    let sql = format!("SELECT {KOLOM} FROM invoice WHERE kode = $1 FOR UPDATE");
    let inv = sqlx::query_as::<_, InvoiceRow>(&sql)
        .bind(&id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if inv.dilunasi_at.is_some() {
        return Err(AppError::Conflict("invoice sudah lunas".into()));
    }
    let denda = denda_invoice(inv.nominal, inv.jatuh_tempo, hari_ini);
    sqlx::query("UPDATE invoice SET dilunasi_at = now(), denda = $2 WHERE id = $1")
        .bind(inv.id)
        .bind(denda)
        .execute(&mut *tx)
        .await?;
    let audit_id = audit::catat_beraudit(
        &mut tx,
        &akun.aktor(),
        "invoice_lunas",
        akun.perangkat_id(),
        json!({"invoice": inv.kode, "nominal": inv.nominal, "denda": denda}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "audit_id": audit_id })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, dd: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, dd).unwrap()
    }

    #[test]
    fn minggu_iso_bolak_balik() {
        // 2026-10-05 adalah Senin minggu ISO ke-41.
        assert_eq!(format_minggu(d(2026, 10, 5)), "2026-W41");
        assert_eq!(urai_minggu("2026-W41"), Some(d(2026, 10, 5)));
        assert_eq!(senin_dari(d(2026, 10, 11)), d(2026, 10, 5)); // Minggu
        assert_eq!(senin_dari(d(2026, 10, 5)), d(2026, 10, 5)); // Senin
                                                                // Batas tahun ISO: 2026-12-31 masuk minggu 53 tahun 2026; 2027-01-01 juga.
        assert_eq!(format_minggu(senin_dari(d(2027, 1, 1))), "2026-W53");
        assert_eq!(urai_minggu("2026-W53"), Some(d(2026, 12, 28)));
    }

    #[test]
    fn minggu_tidak_sah_ditolak() {
        for s in [
            "2026-W00", "2026-W54", "2026-W5", "26-W41", "2026W41", "2026-w41", "abcd-W41",
            "2026-W4a", "",
        ] {
            assert_eq!(urai_minggu(s), None, "{s}");
        }
        // 2027 hanya punya 52 minggu ISO.
        assert_eq!(urai_minggu("2027-W53"), None);
    }

    #[test]
    fn denda_satu_persen_per_minggu_maks_empat() {
        let jt = d(2026, 10, 19);
        let n = 1_000_000;
        assert_eq!(
            denda_invoice(n, jt, d(2026, 10, 19)),
            0,
            "tepat jatuh tempo"
        );
        assert_eq!(
            denda_invoice(n, jt, d(2026, 10, 18)),
            0,
            "sebelum jatuh tempo"
        );
        assert_eq!(
            denda_invoice(n, jt, d(2026, 10, 20)),
            10_000,
            "1 hari lewat = 1 minggu berjalan"
        );
        assert_eq!(
            denda_invoice(n, jt, d(2026, 10, 26)),
            10_000,
            "7 hari lewat"
        );
        assert_eq!(
            denda_invoice(n, jt, d(2026, 10, 27)),
            20_000,
            "8 hari lewat"
        );
        assert_eq!(denda_invoice(n, jt, d(2026, 11, 10)), 40_000, "4 minggu");
        assert_eq!(
            denda_invoice(n, jt, d(2027, 6, 1)),
            40_000,
            "dibatasi 4 persen"
        );
        assert_eq!(denda_invoice(255_000, jt, d(2026, 10, 20)), 2_550);
    }
}
