//! Utilitas waktu: nomor server memakai tanggal WIB (UTC+7).

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};

/// Offset WIB dalam detik.
const WIB_DETIK: i32 = 7 * 3600;

/// Zona WIB sebagai `FixedOffset`.
pub fn wib() -> FixedOffset {
    FixedOffset::east_opt(WIB_DETIK).expect("offset WIB valid")
}

/// Tanggal hari ini menurut WIB.
pub fn hari_ini_wib() -> NaiveDate {
    tanggal_wib(Utc::now())
}

/// Tanggal kalender WIB dari sebuah instan UTC.
pub fn tanggal_wib(waktu: DateTime<Utc>) -> NaiveDate {
    waktu.with_timezone(&wib()).date_naive()
}

/// Selisih durasi hari bantu (agar pemanggil tidak mengimpor `chrono::Duration`).
pub fn hari(n: i64) -> Duration {
    Duration::days(n)
}
