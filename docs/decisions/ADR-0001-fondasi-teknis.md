# ADR-0001 — Fondasi teknis lumbung-backend-service

- Status: Diterima
- Tanggal: 2026-10-14

## Konteks

lumbung-backend-service adalah satu-satunya backend divisi Lumbung dan pemilik DB `lumbung`. Repo sudah berisi templat (Axum, sqlx, JWT, migrasi awal, `docs/auth.md`); fondasi ini mengunci arah sebelum fitur ditulis.

## Keputusan

1. Stack: Rust + Axum `0.8` + Tokio + SQLx Postgres + `sqlx::migrate!` (versi exact di `Cargo.toml`); dilarang mengganti framework/ORM.
2. DB per service: hanya DB logis `lumbung`; tidak ada koneksi ke DB divisi lain. Titeny hanya mengonsumsi event, bukan akses DB.
3. JWT audiens `lumbung`: akses 15 menit (ed25519), refresh 7 hari dengan reuse-detection (satu keluarga dicabut bila refresh dipakai dua kali). Sesuai `docs/auth.md`.
4. Kredensial mesin: API key per perangkat gudang (`X-Device-Key`, hak minimal, rotasi 90 hari dengan masa tenggang 7 hari) dan service key (`X-Service-Key`, least privilege). Perangkat offline menyinkronkan lewat `POST /v1/sinkron/batch` (maks 200 rekaman FIFO) memakai idempotensi UUID klien.
5. Server adalah satu-satunya penghitung fee, grade, dan papan harga; klien dilarang menghitung ulang (FSD/30 Batasan). Fee Dekat Rp1.500/kg, Jauh Rp2.500/kg; pembayaran petani H+1.
6. Galat terpadu `AppError: IntoResponse`; kode baku 400/401/403/404/409/422/429/500 sesuai FSD/30.
7. Kontrak dipublikasikan sebagai OpenAPI 3.1 (`docs/api/openapi.yaml`) + Scalar; basis `https://api.lumbung.chefgenie.id/v1`.

## Konsekuensi

- Semua endpoint baru wajib punya tes `#[sqlx::test]` dan entri OpenAPI.
- Migrasi awal memakai berat `NUMERIC(10,2)` kg dan ID `uuid`; kontrak v1.0.0 mewajibkan gram integer dan kode `L-NNNN`; penyelarasan skema dikerjakan lewat migrasi baru di tiket LMB-BE-004 (bukan edit migrasi lama).
- Pencabutan perangkat/kunci harus bisa dilakukan dalam 1 jam (produk/50).
