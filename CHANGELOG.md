# Changelog

Semua perubahan penting lumbung-backend-service dicatat di sini. Format mengikuti Keep a Changelog; versi mengikuti versi milestone Lumbung.

## [Belum dirilis]

### Ditambahkan
- Auth LMB-BE-003: `POST /v1/auth/masuk|refresh|keluar` (JWT ed25519 15 menit audiens `lumbung`, refresh 7 hari berhash dengan reuse-detection, kunci 15 menit setelah 3 salah PIN), `POST /v1/gudang/masuk|keluar` (token shift 12 jam), ekstraktor `X-Device-Key` (rotasi 90 hari, tenggang 7 hari) dan `X-Service-Key`, log audit `AUD-YYYYMMDD-NNN`, migrasi `0002_auth.sql`, ADR-0002, 22 uji `#[sqlx::test]`.
- Pustaka `lumbung_backend_service` (router di bawah `/v1`), galat baku 409/422/429, ekstraktor `ApiJson` (400 JSON rusak, 422 bentuk salah).
- Kontrak OpenAPI 3.1 v1.0.0 (`docs/api/openapi.yaml`) + referensi Scalar (`docs/api/index.html`) (tiket LMB-BE-002).
- Fondasi AI-dev v1.0.0: `AGENTS.md`, `CLAUDE.md`, `docs/decisions/` (ADR-0001), `versions/v1.0.0/docs/40-TECHSTACK.md`, `versions/v1.0.0/docs/60-BLAST-RADIUS.md` (tiket LMB-BE-001, 2026-10-14).

### Diubah
- Konfigurasi baru `JWT_ED25519_PUBLIC` (PEM SPKI) wajib; `JWT_ED25519_PRIVATE` kini PEM PKCS#8 ed25519. Server lokal OpenAPI menjadi `http://localhost:8080/v1`. Detail galat internal tidak lagi dikirim ke klien (hanya `galat internal`).
