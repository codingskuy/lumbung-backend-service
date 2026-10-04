//! Konfigurasi layanan dibaca dari environment.
//!
//! Variabel wajib:
//! - `DATABASE_URL`: string koneksi Postgres basis data `lumbung`.
//! - `JWT_ED25519_PRIVATE`: kunci privat ed25519 (atau HS256 sementara) penanda JWT.
//! - `DEVICE_API_KEY_GUDANG`: kunci API perangkat gudang.
//! - `SERVICE_API_KEY`: kunci API antar-layanan.

use anyhow::Context;

/// Konfigurasi runtime layanan backend Lumbung.
#[derive(Debug, Clone)]
pub struct Config {
    /// Alamat dengar HTTP, mis. `0.0.0.0:8080`.
    pub listen_addr: String,
    /// String koneksi Postgres basis data `lumbung`.
    pub database_url: String,
    /// Kunci penanda JWT (ed25519 privat).
    pub jwt_private_key: String,
    /// Kunci API perangkat gudang (timbangan/cetak).
    pub device_api_key: String,
    /// Kunci API antar-layanan (pipeline, AI serving).
    pub service_api_key: String,
}

impl Config {
    /// Baca konfigurasi dari environment; gagal bila variabel wajib kosong.
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            listen_addr: std::env::var("LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL wajib diisi")?,
            jwt_private_key: std::env::var("JWT_ED25519_PRIVATE")
                .context("JWT_ED25519_PRIVATE wajib diisi")?,
            device_api_key: std::env::var("DEVICE_API_KEY_GUDANG")
                .context("DEVICE_API_KEY_GUDANG wajib diisi")?,
            service_api_key: std::env::var("SERVICE_API_KEY")
                .context("SERVICE_API_KEY wajib diisi")?,
        })
    }
}
