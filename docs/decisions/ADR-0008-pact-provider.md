# ADR-0008 — Verifikasi provider Pact dan fallback KMP (LMB-BE-TEST-PACT)

- Status: Diterima
- Tanggal: 2026-10-22
- Catatan: issue #13 menyebut "ADR-0002" untuk fallback KMP; nomor itu sudah dipakai (auth), sehingga dicatat di sini.

## Konteks

Backend adalah provider untuk kontrak konsumen web (lumbung-web) dan app-kmp. Berkas pact web tersedia (lumbung-web PR #16, Pact v3, pact-js 17.1.4); berkas app-kmp belum ada (LMB-KMP-TEST-PACT, app-kmp #8).

## Keputusan

1. **Harness** `contract/pact/verifikasi.mjs` memakai pact-js `Verifier` (pustaka native Pact Rust) dengan Node, bukan JVM atau CLI Ruby: satu bahasa untuk berkas pact bahasa apa pun (format pact bersifat netral konsumen), tanpa JDK/Gradle di repo Rust. State handler memakai `psql` dan API backend sendiri; hanya untuk DB uji.
2. **Berkas pact di-commit** (`contract/pact/pacts/`), tanpa Pact Broker (sesuai catatan konsumen web: broker hanya dokumentasi). Sinkron manual dari repo konsumen.
3. **Hasil nyata**: 4 interaksi pact web diverifikasi terhadap server nyata dan lulus (papan harga publik 200 dan 404, profil 200 dan 500). Verifikasi pertama menemukan satu ketidakcocokan nyata: pact web mengharapkan `deskripsi` dan `kontak_kemitraan` pada `GET /publik/profil`, sedangkan implementasi sebelumnya hanya mengembalikan `nama`; juga state "basis data tidak tersedia" tidak bisa disimulasikan karena profil berupa konstanta. Diperbaiki: profil dibaca dari tabel `profil_publik` (migrasi 0007; deskripsi dan kontak opsional, diisi operator via SQL, tanpa endpoint tulis sesuai ADR-0006).
4. **Fallback KMP (bila Pact di KMP tidak layak):** pact-jvm hanya berjalan di target JVM/Android; target Kotlin/Native (iOS) tidak punya pustaka Pact. Rencana: konsumen app-kmp menulis pact dari uji `jvmTest`/`androidUnitTest` (mock server pact-jvm), keluarannya berkas Pact v3 yang sama dan cukup ditaruh di `contract/pact/pacts/` untuk diverifikasi harness ini. Bila pact-jvm tidak dapat dipakai di modul KMP (versi Kotlin/Gradle), fallback berikutnya adalah Schemathesis (ADR-0007) yang sudah menguji seluruh `openapi.yaml` dari sisi provider, ditambah uji kontrak klien Ktor terhadap contoh respons OpenAPI di app-kmp. Keputusan akhir menunggu berkas atau laporan app-kmp #8.
5. **Analitik**: `npm install --ignore-scripts` mematikan postinstall analitik (scarf); tidak ada data keluar.

## Konsekuensi

- Menambah konsumen = menaruh berkas pact dan menambah state handler yang belum dikenal.
- CI manual `Pact provider (manual)` ditulis tetapi belum dijalankan di GitHub Actions.
