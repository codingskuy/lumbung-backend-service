> Versi: v1.0.0 | Status: disetujui | Menggantikan: -
> Sumber: Lumbung-TownHall `platform/versions/v1.0.0/` pada commit `673a467` ([platform/versions/v1.0.0](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/673a467126e195ffd67c9d3ff21d108193a6c3ec/platform/versions/v1.0.0))

# 40 — Tech Stack lumbung-backend-service

Matriks stack terkunci backend. Versi diambil dari `Cargo.toml` pada main per 2026-10-14.

## Matriks

| Komponen | Versi | Peran | Catatan |
|---|---|---|---|
| Rust | edisi 2021 | Bahasa | toolchain stabil terpasang di CI |
| axum | 0.8.4 | HTTP | path `{id}` (bukan `:id`) |
| tokio | 1.47.1 | Runtime | feature `full`; `test-util` untuk dev |
| sqlx | 0.8.6 | Postgres | `cargo sqlx prepare`; `#[sqlx::test]` |
| tower-http | 0.6.2 | Middleware | trace, cors, timeout (10 dtk) |
| tracing / tracing-subscriber | 0.1.41 / 0.3.19 | Observabilitas | `EnvFilter` |
| serde / serde_json | 1.0.219 / 1.0.143 | Serialisasi | |
| validator | 0.20.0 | Validasi input | berat >= 0, total sortir +-2% |
| uuid | 1.18.0 | ID | v7 server, v4 klien |
| chrono | 0.4.41 | Waktu | offset +07:00 pada kontrak |
| jsonwebtoken | 9.3.1 | JWT ed25519 | `JWT_ED25519_PRIVATE` |
| argon2 | 0.5.3 | Hash PIN/kata sandi | PIN 6 digit, ganti 90 hari |
| thiserror / anyhow | 2.0.12 / 1.0.99 | Galat | satu `AppError` |

## Konfigurasi runtime

`DATABASE_URL`, `JWT_ED25519_PRIVATE`, `DEVICE_API_KEY_GUDANG`, `SERVICE_API_KEY` (lihat `README.md` dan `lumbung-infra-devops`). Tidak pernah di-commit.

## Titik layanan

Basis publik `https://api.lumbung.chefgenie.id/v1`; lokal `http://localhost:8080`. Kontrak: `docs/api/openapi.yaml`.
