//! Pustaka layanan backend Lumbung (Axum).
//!
//! `main.rs` hanya merakit konfigurasi dan menyalakan server; router dan semua logika
//! ada di sini agar bisa diuji `#[sqlx::test]` lewat `tower::ServiceExt::oneshot`.

pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod extract;
pub mod routes;
pub mod state;
pub mod urutan;
pub mod waktu;

pub use routes::router;
