# Changelog

Semua perubahan penting lumbung-backend-service dicatat di sini. Format mengikuti Keep a Changelog; versi mengikuti versi milestone Lumbung.

## [Belum dirilis]

### Ditambahkan
- Keuangan LMB-BE-006: fee per kg (Rp1.500 dekat, Rp2.500 jauh; `fee_total` dan `rincian_fee` pada surat jalan), invoice fee mingguan per pembeli (`GET /v1/admin/invoice`, terbit Senin, jatuh tempo 7 hari, denda 1%/minggu maks 4%, tugas latar per jam), `POST /v1/admin/invoice/{id}/lunas` (beraudit, denda dibekukan), pembayaran petani H+1 (`GET /v1/pembayaran-saya`, difilter server, denda 0,5%/hari maks 5%); migrasi `0006_fee_invoice_pembayaran.sql`, ADR-0005; 13 uji integrasi + 8 uji unit.
- Trip dan sinkron LMB-BE-005: `POST /v1/trip` (`TRIP-RUTE-NNN`, kas jalan Rp300.000, titik/sopir/tanggal opsional), `POST /v1/trip/{id}/surat-jalan` (`SJ-YYYYMMDD-NNN`, TMP-* dipetakan), `POST /v1/sinkron/batch` (maks 200, FIFO, savepoint per rekaman, KONFLIK gudang menang, kunci offline 3x24 jam), endpoint sopir `GET /v1/trip-hari-ini`, `POST /v1/titik/{id}/tiba|berangkat`, `POST /v1/bukti-terima`, `POST /v1/unggah-foto` (JPEG maks 500 KB), `POST /v1/insiden`; login sopir memuat `trip_hari_ini`; migrasi `0005_trip_sinkron.sql`, ADR-0004; 21 uji integrasi baru.
- Lot dan sortir LMB-BE-004: `POST /v1/lot` (kode `L-NNNN` dari server), `POST /v1/lot/{id}/sortir` (gram integer, toleransi 2 persen, Grade C dialihkan dan dicatat), `POST /v1/keranjang/batch` (maks 50, atomik, stok terkunci), `GET /v1/stok-lot`; semua POST idempoten via UUID klien; migrasi `0004_lot_sortir.sql` (menyelaraskan skema templat 0001), ADR-0003; 15 uji integrasi + 3 uji unit.
- Papan harga LMB-BE-007: `POST /v1/admin/papan-harga` (3 komoditas x grade A/B + fee 2 zona dari server, idempoten, beraudit `audit_id`), `GET /v1/publik/papan-harga` (tanpa login, `Cache-Control: public, max-age=900`), `GET /v1/papan-harga` (petani, baca saja), `GET /v1/kesehatan`; migrasi `0003_papan_harga.sql`; 9 uji integrasi + 3 uji formula.
- Auth LMB-BE-003: `POST /v1/auth/masuk|refresh|keluar` (JWT ed25519 15 menit audiens `lumbung`, refresh 7 hari berhash dengan reuse-detection, kunci 15 menit setelah 3 salah PIN), `POST /v1/gudang/masuk|keluar` (token shift 12 jam), ekstraktor `X-Device-Key` (rotasi 90 hari, tenggang 7 hari) dan `X-Service-Key`, log audit `AUD-YYYYMMDD-NNN`, migrasi `0002_auth.sql`, ADR-0002, 22 uji `#[sqlx::test]`.
- Pustaka `lumbung_backend_service` (router di bawah `/v1`), galat baku 409/422/429, ekstraktor `ApiJson` (400 JSON rusak, 422 bentuk salah).
- Kontrak OpenAPI 3.1 v1.0.0 (`docs/api/openapi.yaml`) + referensi Scalar (`docs/api/index.html`) (tiket LMB-BE-002).
- Fondasi AI-dev v1.0.0: `AGENTS.md`, `CLAUDE.md`, `docs/decisions/` (ADR-0001), `versions/v1.0.0/docs/40-TECHSTACK.md`, `versions/v1.0.0/docs/60-BLAST-RADIUS.md` (tiket LMB-BE-001, 2026-10-14).

### Diubah
- Keputusan pemilik produk (ADR-0006): Grade B dibayar 85 persen harga papan (contoh L-8812 menjadi Rp3.524.850); `POST /admin/invoice/{id}/lunas` hanya `admin_keuangan` atau kunci layanan; `.env.example` dengan lima rahasia tanpa nilai; uji skema memastikan berat gram integer (tanpa NUMERIC kg).
- Pemeriksaan peran kini dilakukan sebelum badan permintaan dibaca (ekstraktor `OperatorGudang`, `PeranSopir`, `AdminGudangAkses`): peran salah selalu 403, bukan 422. Fitur `multipart` pada `axum` diaktifkan untuk unggah foto.
- Konfigurasi baru `JWT_ED25519_PUBLIC` (PEM SPKI) wajib; `JWT_ED25519_PRIVATE` kini PEM PKCS#8 ed25519. Server lokal OpenAPI menjadi `http://localhost:8080/v1`. Detail galat internal tidak lagi dikirim ke klien (hanya `galat internal`).
