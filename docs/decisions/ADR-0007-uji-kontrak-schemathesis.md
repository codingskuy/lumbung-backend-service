# ADR-0007 — Uji kontrak Schemathesis dan temuannya (LMB-BE-TEST-SCHEMATHESIS)

- Status: Diterima
- Tanggal: 2026-10-22
- Catatan penomoran: issue #14 menyebut "ADR-0002" untuk keputusan tooling, tetapi ADR-0002 sudah dipakai untuk auth (LMB-BE-003); ADR ini memakai nomor berikutnya.

## Konteks

Server nyata belum pernah dijalankan (hanya `#[sqlx::test]` lewat `oneshot`). Siklus 2 menjalankan `main.rs` dengan DB lokal dan menguji `docs/api/openapi.yaml` dengan Schemathesis 4.32.2 (via `uvx`, telemetri mati).

## Hasil nyata

Boot: `cargo run` dengan lima variabel lingkungan berhasil (migrasi 0001-0006, kunci perangkat terdaftar, `/healthz` dan `/v1/kesehatan` 200). Schemathesis dijalankan per peran terhadap server lokal; pass awal menemukan antara lain:

1. Rute tak dikenal/metode tak didukung menjawab 404/405 tanpa badan dan tanpa `Content-Type` (ST104/ST109/ST204). Diperbaiki: fallback JSON baku (`Galat`) untuk 404 dan 405.
2. Bearer rusak diterima bila `X-Service-Key` sah dikirim bersamaan (ST401). Diperbaiki: semua kredensial yang dikirim harus sah.
3. Skema permintaan lebih longgar daripada validasi server (string kosong, `KRJ-000`/`KRJ-121`, berat 0, lat/lng di luar rentang, bidang tak dikenal, `kas_jalan` selain 300000, minggu ISO tak sah) dan sebaliknya server lebih longgar daripada skema (bidang tak dikenal diterima pada `/auth/masuk`, `/gudang/masuk`, `/auth/refresh`; nilai `null` diterima pada bidang opsional; payload sinkron bukan objek dan `nomor_sementara` salah format diterima per rekaman; permintaan melanggar jumlah foto lolos sebagai duplikat karena idempotensi dicek lebih dulu). Diperbaiki di dua sisi: skema diperketat (`additionalProperties: false`, `minLength`, pola KRJ 001-120, `GramPositif`, lat/lng, minggu) dan server menolak (`deny_unknown_fields`, tolak `null`, 422 untuk payload bukan objek dan `nomor_sementara` salah, validasi foto sebelum idempotensi).
4. Respons 400 (badan tak terbaca) dan 422 (bentuk salah) tidak terdokumentasi pada banyak operasi. Diperbaiki: komponen `InputTakTerbaca` dan `Validasi` pada semua operasi ber-badan.
5. Tujuh endpoint ada di kontrak tetapi tidak ada di server (404): `GET /publik/profil`, `/admin/rekap-harian`, `/admin/mitra`, `/admin/insiden`, `/kontrak-saya`, `POST /rencana-setoran`, `/admin/mitra/{id}/status`. Keputusan: empat endpoint baca diimplementasi (`src/admin.rs`; profil hanya `nama` karena konten belum didefinisikan); tiga lainnya menjawab **501 terdokumentasi** (`BelumDiimplementasi`) karena entitas kontrak musim belum ada dan penulisan admin selain papan harga dan invoice lunas menunggu tiket (ADR-0006).
6. Fuzzing `POST /gudang/keluar` mencabut token shift sehingga operasi lain pada pass yang sama hanya mendapat 401 (cakupan hilang diam-diam). Diperbaiki di skrip: dijalankan sebagai pass terpisah di akhir.

Hasil akhir: `contract/schemathesis.sh` dengan 100 contoh per operasi selesai tanpa kegagalan (7 pass; 5xx tidak ditemukan selain 501 yang dikecualikan).

## Keputusan

- Pengecualian yang disengaja: check `positive_data_acceptance` (data lolos skema tetapi ditolak aturan domain, mis. total sortir di luar 2 persen, `berlaku_mulai` bukan Senin, foto belum diunggah, `lot_id` berbeda dari path) karena 422 terdokumentasi; tiga operasi 501.
- Telemetri: Schemathesis 4 tidak punya opsi telemetri pada berkas konfigurasi; tidak ada akun/token layanan eksternal yang dipakai, dan `SCHEMATHESIS_TELEMETRY=off` diset.
- Seed uji (`contract/seed.py`) hanya untuk DB uji; PIN contoh bukan rahasia. Pengerasan akun terkunci oleh fuzzing: seed ulang membuka kunci.
- CI manual `Schemathesis (manual)` (`workflow_dispatch`) disediakan namun BELUM dijalankan di GitHub Actions.

## Konsekuensi

- Kontrak kini lebih ketat; klien yang mengirim bidang tak dikenal atau `null` untuk bidang opsional akan mendapat 422.
- Pass per peran memakai akun seed; perubahan peran endpoint harus memperbarui `contract/schemathesis.sh`.
