# Lumbung-Backend
ini adalah aplikasi backend dimana logika dan bisnis dituangkan dalam kode

## Template Opsi A — Rust Axum + Auth + DB lumbung

Layanan API Lumbung untuk rantai pasok hortikultura: panen, lot, trip, timbangan, dan fee.

### Teknologi

- Rust + Axum `0.8`, Tokio `1` (full), `sqlx` `0.8` (Postgres + uuid + migrate).
- `tower-http` (trace/cors/timeout), `tracing`, `serde`/`serde_json`, `thiserror` + `anyhow`.
- `uuid` v7, `validator`, `jsonwebtoken`, `argon2`.
- Basis data: Postgres khusus `lumbung` (terpisah dari divisi lain).

### Struktur

```text
lumbung-backend-service/
├── src/main.rs        # router + GET /healthz (cek DB)
├── src/config.rs      # baca env (DATABASE_URL lumbung, JWT ed25519, API key)
├── src/error.rs       # galat terpadu -> HTTP
├── src/state.rs       # AppState (pool + config)
├── src/db.rs          # koneksi pool sqlx + tuning
├── migrations/0001_init.sql  # tabel panen/lot/trip/timbangan/fee
└── docs/auth.md       # JWT 15 mnt + refresh 7 hari + device/service key
```

### Cara jalan

```bash
export DATABASE_URL=postgres://lumbung_app:rahasia@localhost:5432/lumbung
export JWT_ED25519_PRIVATE=isi-kunci-privat
export DEVICE_API_KEY_GUDANG=isi-kunci-gudang
export SERVICE_API_KEY=isi-kunci-layanan
cargo sqlx prepare
cargo test --locked
cargo run
curl localhost:8080/healthz
```

### Keterkaitan

- Platform: [Lumbung-TownHall](https://github.com/Coding-Skuy/Lumbung-TownHall)
- Web: [lumbung-web](https://github.com/Coding-Skuy/lumbung-web)
- Pipeline: [lumbung-data-pipeline](https://github.com/Coding-Skuy/lumbung-data-pipeline)
