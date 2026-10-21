//! Log audit: siapa, perangkat, waktu (produk/50 keputusan 5).
//!
//! `catat` untuk peristiwa biasa (login, penolakan 403); `catat_beraudit` untuk
//! penulisan admin yang wajib mengembalikan `audit_id` (`AUD-YYYYMMDD-NNN`).

use serde_json::Value;
use sqlx::PgConnection;

use crate::{urutan, waktu};

/// Catat peristiwa audit tanpa `audit_id`.
pub async fn catat(
    conn: &mut PgConnection,
    aktor: &str,
    aksi: &str,
    perangkat: Option<&str>,
    detail: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_log (aktor, aksi, perangkat, detail) VALUES ($1, $2, $3, $4)")
        .bind(aktor)
        .bind(aksi)
        .bind(perangkat)
        .bind(detail)
        .execute(conn)
        .await?;
    Ok(())
}

/// Catat penulisan beraudit dan kembalikan `audit_id` (`AUD-YYYYMMDD-NNN`).
pub async fn catat_beraudit(
    conn: &mut PgConnection,
    aktor: &str,
    aksi: &str,
    perangkat: Option<&str>,
    detail: Value,
) -> Result<String, sqlx::Error> {
    let tanggal = waktu::hari_ini_wib();
    let urut = urutan::berikutnya(&mut *conn, "AUD", tanggal).await?;
    let audit_id = urutan::format_nomor("AUD", tanggal, urut);
    sqlx::query(
        "INSERT INTO audit_log (audit_id, aktor, aksi, perangkat, detail) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&audit_id)
    .bind(aktor)
    .bind(aksi)
    .bind(perangkat)
    .bind(detail)
    .execute(conn)
    .await?;
    Ok(audit_id)
}
