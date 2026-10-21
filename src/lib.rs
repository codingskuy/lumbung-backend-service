//! Pustaka layanan backend Lumbung (Axum).
//!
//! `main.rs` hanya merakit konfigurasi dan menyalakan server; router dan semua logika
//! ada di sini agar bisa diuji `#[sqlx::test]` lewat `tower::ServiceExt::oneshot`.

pub mod admin;
pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod extract;
pub mod fee;
pub mod invoice;
pub mod keranjang;
pub mod lot;
pub mod papan_harga;
pub mod pembayaran;
pub mod routes;
pub mod sinkron;
pub mod sistem;
pub mod sopir;
pub mod state;
pub mod trip;
pub mod urutan;
pub mod waktu;

pub use routes::router;
