## Ringkasan
<!-- Apa yang berubah dan mengapa (2-5 butir). -->

## Tiket
Closes #

## Ketergantungan
<!-- "Bergantung pada #N, merge berurutan" bila cabang bertumpuk; kosongkan bila tidak. -->

## Perubahan kontrak (konsumen web/app)
<!-- Ringkas perubahan `docs/api/openapi.yaml`, atau tulis "tidak ada". Endpoint baru/berubah WAJIB memperbarui OpenAPI di PR ini. -->

## Cara uji
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
DATABASE_URL=postgres://... cargo test
```

## Bukti
<!-- Keluaran uji nyata (jumlah lulus/gagal). Pisahkan: DIJALANKAN vs HANYA DITULIS. -->

## Definisi selesai
- [ ] Tes ditulis bersama kode; fmt, clippy `-D warnings`, dan test hijau
- [ ] `docs/api/openapi.yaml` diperbarui bila endpoint berubah (YAML valid)
- [ ] ADR baru bila keputusan arsitektur/auth; `docs/auth.md` bila auth
- [ ] `CHANGELOG.md` diperbarui
- [ ] `graphify update .` dijalankan (graphify-out tidak di-commit)
- [ ] Tidak ada rahasia; migrasi lama tidak diedit

## Risiko / blast-radius
<!-- Migrasi, perubahan perilaku, dampak ke konsumen, asumsi yang perlu ditinjau. -->

🤖 Generated with [Claude Code](https://claude.com/claude-code)
