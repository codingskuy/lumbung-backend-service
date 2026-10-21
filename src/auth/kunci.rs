//! API key perangkat gudang (`X-Device-Key`): hanya hash SHA-256 yang disimpan.
//!
//! Aturan (docs/auth.md, ADR-0001 butir 4): berlaku 90 hari sejak dibuat; saat rotasi,
//! kunci lama tetap berlaku 7 hari (masa tenggang) lalu mati. Hak minimal: tulis
//! `timbangan` dan `fee.status_cetak`, tanpa baca data pengguna.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Masa berlaku kunci perangkat (hari) sebelum wajib dirotasi.
pub const MASA_BERLAKU_HARI: i64 = 90;
/// Masa tenggang kunci lama setelah rotasi (hari).
pub const TENGGANG_HARI: i64 = 7;

/// Hak yang dimiliki kunci perangkat gudang; selain ini ditolak.
pub const HAK_PERANGKAT: &[&str] = &["timbangan:tulis", "fee.status_cetak:tulis"];

/// Kunci perangkat yang lolos verifikasi.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KunciPerangkat {
    /// ID baris kunci.
    pub id: Uuid,
    /// Label perangkat/gudang (contoh `GUDANG-01`).
    pub label: String,
    /// Batas akhir berlaku.
    pub berlaku_sampai: DateTime<Utc>,
}

/// Cari kunci aktif (belum dicabut, belum lewat `berlaku_sampai`).
pub async fn verifikasi(pool: &PgPool, kunci: &str) -> Result<Option<KunciPerangkat>, sqlx::Error> {
    sqlx::query_as::<_, KunciPerangkat>(
        "SELECT id, label, berlaku_sampai FROM kunci_perangkat
         WHERE key_hash = encode(digest($1, 'sha256'), 'hex')
           AND dicabut_at IS NULL AND berlaku_sampai > now()",
    )
    .bind(kunci)
    .fetch_optional(pool)
    .await
}

/// Rotasi: daftarkan kunci baru (90 hari) dan persingkat kunci lama sekeranjang label
/// menjadi paling lama 7 hari dari sekarang. Mengembalikan ID kunci baru.
pub async fn rotasi(pool: &PgPool, label: &str, kunci_baru: &str) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let sekarang = Utc::now();
    let tenggang = sekarang + Duration::days(TENGGANG_HARI);
    sqlx::query(
        "UPDATE kunci_perangkat SET berlaku_sampai = LEAST(berlaku_sampai, $2)
         WHERE label = $1 AND dicabut_at IS NULL",
    )
    .bind(label)
    .bind(tenggang)
    .execute(&mut *tx)
    .await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO kunci_perangkat (label, key_hash, dibuat_at, berlaku_sampai)
         VALUES ($1, encode(digest($2, 'sha256'), 'hex'), $3, $4) RETURNING id",
    )
    .bind(label)
    .bind(kunci_baru)
    .bind(sekarang)
    .bind(sekarang + Duration::days(MASA_BERLAKU_HARI))
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Pastikan kunci dari environment terdaftar; bila belum ada, dirotasi masuk
/// (kunci lama label yang sama mendapat tenggang 7 hari). Idempoten saat restart.
pub async fn pastikan_kunci_awal(
    pool: &PgPool,
    label: &str,
    kunci: &str,
) -> Result<(), sqlx::Error> {
    let ada: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM kunci_perangkat WHERE key_hash = encode(digest($1, 'sha256'), 'hex'))",
    )
    .bind(kunci)
    .fetch_one(pool)
    .await?;
    if !ada {
        rotasi(pool, label, kunci).await?;
    }
    Ok(())
}

/// Cabut kunci seketika (perangkat hilang, target di bawah 1 jam).
pub async fn cabut(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let hasil = sqlx::query(
        "UPDATE kunci_perangkat SET dicabut_at = now() WHERE id = $1 AND dicabut_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(hasil.rows_affected() > 0)
}
