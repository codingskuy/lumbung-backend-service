//! Penghitung urutan harian atomik untuk nomor server
//! (`AUD-YYYYMMDD-NNN`, `SJ-YYYYMMDD-NNN`, `INS-YYYYMMDD-NNN`).

use chrono::NaiveDate;
use sqlx::PgConnection;

/// Ambil nomor urut berikutnya (mulai 1) untuk `jenis` pada `tanggal`.
///
/// Memakai upsert sehingga aman terhadap permintaan bersamaan; bila dipanggil
/// di dalam transaksi, penomoran ikut dibatalkan saat transaksi gagal.
pub async fn berikutnya(
    conn: &mut PgConnection,
    jenis: &str,
    tanggal: NaiveDate,
) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar::<_, i32>(
        "INSERT INTO urutan_harian (jenis, tanggal, nilai) VALUES ($1, $2, 1)
         ON CONFLICT (jenis, tanggal) DO UPDATE SET nilai = urutan_harian.nilai + 1
         RETURNING nilai",
    )
    .bind(jenis)
    .bind(tanggal)
    .fetch_one(conn)
    .await
}

/// Penghitung permanen (tidak direset harian), contoh nomor trip per rute. Mulai dari 1.
pub async fn berikutnya_permanen(conn: &mut PgConnection, jenis: &str) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar::<_, i32>(
        "INSERT INTO penghitung (jenis, nilai) VALUES ($1, 1)
         ON CONFLICT (jenis) DO UPDATE SET nilai = penghitung.nilai + 1
         RETURNING nilai",
    )
    .bind(jenis)
    .fetch_one(conn)
    .await
}

/// Format `<AWALAN>-YYYYMMDD-NNN`.
pub fn format_nomor(awalan: &str, tanggal: NaiveDate, urut: i32) -> String {
    format!("{awalan}-{}-{urut:03}", tanggal.format("%Y%m%d"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_nomor_tiga_digit() {
        let t = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        assert_eq!(format_nomor("AUD", t, 3), "AUD-20261009-003");
        assert_eq!(format_nomor("SJ", t, 120), "SJ-20261009-120");
    }
}
