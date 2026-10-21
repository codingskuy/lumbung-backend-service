# Panduan kontribusi — lumbung-backend-service

Backend Rust (Axum) divisi Lumbung. Aturan teknis terkunci ada di [`AGENTS.md`](./AGENTS.md); dokumen ini ringkasan alur kerja.
Bahasa artefak (issue, komentar, commit, PR, dokumen): Indonesia.

## Sebelum menulis kode

1. Baca `AGENTS.md`, semua ADR di `docs/decisions/`, dan tiket (issue) beserta tiket induknya di Lumbung-TownHall.
2. Requirement tidak diubah di repo ini: perubahan requirement dimulai dari PR di Lumbung-TownHall. Versi beku (`versions/v*`) tidak diedit.
3. Angka bisnis (tarif, ambang, persentase) hanya dari dokumen; berat selalu gram integer, uang rupiah integer.

## Alur cabang dan PR

- Fork repo, buat cabang dari `main`: `feat/<kode-tiket>-<slug>`, `fix/...`, `test/...`, `docs/...`, `chore/...`. Satu cabang = satu tiket = satu PR.
- Cabang yang bergantung pada PR yang belum merge boleh bertumpuk; tulis "Bergantung pada #N, merge berurutan" di PR.
- Commit atomik (satu perubahan logis), awalan konvensional (`feat:`, `fix:`, `test:`, `docs:`, `chore:`, `refactor:`), pesan Indonesia.
- Jangan force-push ke `main`, jangan merge PR sendiri, jangan menutup issue manual (pakai `Closes #N` di PR).
- Jangan commit rahasia (`.env`, kunci). Salin `.env.example` ke `.env` untuk pengembangan lokal.

## Definisi selesai tiap PR

- [ ] Tes ditulis bersama kode (TDD merah-hijau bila praktis); `#[sqlx::test]` untuk alur basis data.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, dan `cargo test` hijau (butuh Postgres dengan hak `createdb`; `DATABASE_URL` menunjuk server, bukan basis data produksi).
- [ ] Perubahan endpoint memperbarui `docs/api/openapi.yaml` dalam PR yang sama (handler tanpa entri OpenAPI tidak boleh digabung); YAML divalidasi.
- [ ] Perubahan auth atau keputusan arsitektur: ADR baru bernomor berikutnya (ADR lama tidak diedit; "Menggantikan ADR-NNNN"), serta `docs/auth.md` bila auth.
- [ ] `CHANGELOG.md` diperbarui (Indonesia, bagian "Belum dirilis").
- [ ] `graphify update .` dijalankan setelah mengubah kode (AST-only); `graphify-out/` tidak di-commit.
- [ ] Deskripsi PR: Ringkasan, tiket (`Closes #N`), cara uji, bukti (keluaran uji), risiko, dan apa yang BELUM dijalankan.

## Menjalankan lokal

```bash
cp .env.example .env            # isi lima rahasia; JANGAN commit
export DATABASE_URL=postgres://$USER@localhost:5432/lumbung_dev
cargo test && cargo clippy --all-targets -- -D warnings
```

Uji kontrak (Schemathesis) dan verifikasi provider Pact: lihat `contract/README.md` dan `contract/pact/README.md` setelah PR LMB-BE-TEST-* digabung.

## Larangan

Framework web selain Axum, ORM selain SQLx, memperbarui versi dependensi tanpa ADR, query ke DB divisi lain, rahasia di repo, mengedit migrasi yang sudah ada (buat migrasi baru).
