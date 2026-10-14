# AGENTS.md — lumbung-backend-service

Backend Rust (Axum) tunggal Lumbung: sumber kebenaran logika fee/kg, sortir, trip, papan harga, pembayaran petani H+1, dan auth divisi (`docs/auth.md`).

## Sumber kebenaran

Requirement hidup di [Lumbung-TownHall](https://github.com/Coding-Skuy/Lumbung-TownHall), bukan di repo ini. Dilarang menduplikasi requirement di repo kode; requirement berubah = PR di Lumbung-TownHall dulu (versi beku `v1.0.0` tidak diedit; perubahan masuk v1.1.0/v2.0.0).

| Dokumen | Isi yang berlaku untuk repo ini |
|---|---|
| [`platform/versions/v1.0.0/FSD/30-kontrak.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/FSD/30-kontrak.md) | Kontrak API, event, galat baku (sumber OpenAPI `docs/api/openapi.yaml`) |
| [`platform/versions/v1.0.0/FSD/20-model-data.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/FSD/20-model-data.md) | Entitas, enum, aturan angka (gram/rupiah integer) |
| [`platform/versions/v1.0.0/FSD/10-alur.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/FSD/10-alur.md) | Urutan tulis-lokal-dahulu, nomor TMP, resolusi konflik, batas darurat offline |
| [`platform/versions/v1.0.0/FRD/10-fungsional.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/FRD/10-fungsional.md) | FR-001..FR-303 |
| [`platform/versions/v1.0.0/BRD/30-keuangan-fee.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/BRD/30-keuangan-fee.md) | BR-301..BR-306: fee, invoice, papan harga |
| [`platform/versions/v1.0.0/PRD/10-pengguna.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/PRD/10-pengguna.md) | Peran dan hak akses (1 orang 1 akun) |
| [`platform/versions/v1.0.0/SRS/30-non-functional.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/SRS/30-non-functional.md) | Keamanan (JWT audiens, least privilege), DB per service |
| [`platform/versions/v1.0.0/SRS/40-acceptance-criteria.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/platform/versions/v1.0.0/SRS/40-acceptance-criteria.md) | QA-001..QA-004 |
| [`produk/30-kontrak-api-KMP-mobile.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/produk/30-kontrak-api-KMP-mobile.md) | Endpoint sopir (5) + petani (4) |
| [`produk/31-kontrak-api-desktop-windows.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/produk/31-kontrak-api-desktop-windows.md) | Endpoint gudang (7) |
| [`produk/32-kontrak-api-web-bun.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/produk/32-kontrak-api-web-bun.md) | Endpoint web publik (2) + admin (4 baca + 3 tulis) |
| [`produk/50-autentikasi-perangkat.md`](https://github.com/Coding-Skuy/Lumbung-TownHall/blob/main/produk/50-autentikasi-perangkat.md) | Tiga lapis kredensial, pendaftaran dan pencabutan perangkat |
| [`docs/auth.md`](./docs/auth.md) | Sumber kebenaran auth di repo ini (JWT, `X-Device-Key`, `X-Service-Key`); diselaraskan, tidak ditimpa |
| [`KONTEKS-LINTAS-DIVISI.md`](https://github.com/Coding-Skuy/ChefGenie-TownHall/blob/main/KONTEKS-LINTAS-DIVISI.md) (ChefGenie-TownHall) | Event registry §3, komoditas pilot §4, grade §5, satuan/ID/uang §6 |
| [`townhall-roadmap.md`](https://github.com/Coding-Skuy/ChefGenie-TownHall/blob/main/townhall-roadmap.md) (ChefGenie-TownHall) | Batas context §2, Definisi Selesai §6 |

Angka terkunci v1.0.0 yang berlaku di seluruh repo Lumbung:

- 3 komoditas pilot: cabai, bawang, kangkung (KONTEKS §4; dilarang mencampur angka 3 dan 12 komoditas tanpa label fase).
- Fee logistik murni: Dekat (<15 km) Rp1.500/kg, Jauh (15-25 km) Rp2.500/kg; radius layanan 25 km (BRD/30 BR-301).
- Uang integer rupiah, berat integer gram; pembayaran petani H+1 dari berat bersih per grade x harga papan.
- Papan harga dihitung Jumat 15.00 (median 3 referensi - 10%, bulatkan ke bawah kelipatan Rp500), publikasi maksimal 17.00.
- Titik impas 18.000 kg/bulan (600 kg/hari); target 2 ton/hari; susut total <5%.

## Tech stack terkunci

| Lapisan | Pilihan terkunci |
|---|---|
| Bahasa/Framework | Rust edisi 2021, Axum `0.8.4` (path parameter `{id}`), Tokio `1.47.1` (full) |
| Basis data | Postgres, DB logis `lumbung`; SQLx `0.8.6` (postgres, uuid, migrate, chrono) + `sqlx::migrate!` |
| Middleware | `tower-http` `0.6.2` (Trace + Timeout + CORS), `tracing` `0.1.41` |
| Validasi/ID | `validator` `0.20.0`, `uuid` `1.18.0` (v7 untuk ID server) |
| Auth | `jsonwebtoken` `9.3.1` (ed25519), `argon2` `0.5.3` |
| Pola kode | `AppState { pool: PgPool, config }` via `State`, satu `AppError: IntoResponse`, tes dengan `#[sqlx::test]` |

Larangan: ORM selain SQLx, framework web selain Axum, query ke DB divisi lain, rahasia di repo, memperbarui versi dependensi tanpa ADR.

## Aturan kerja AI

1. **Baca dulu.** Baca `AGENTS.md` ini dan seluruh ADR di `docs/decisions/` sebelum menulis kode.
2. **TDD merah-hijau.** Tulis tes gagal dari acceptance criteria tiket, lalu implementasi. Rilis diblokir bila QA gate gagal (SRS `40-acceptance-criteria.md`: QA-001..QA-004; tidak ada tes yang di-skip).
3. **Kontrak berversi.** Kontrak API/event hanya diubah lewat bump versi + ADR baru. Perubahan requirement dimulai dari PR di Lumbung-TownHall, bukan dari repo ini.
4. **Commit.** Awalan `docs:` / `feat:` / `fix:`; setiap PR wajib memperbarui `CHANGELOG.md` repo ini.
5. **ADR immutable.** Keputusan arsitektur baru = ADR bernomor berikutnya di `docs/decisions/`; ADR lama tidak diedit (diganti dengan ADR baru berstatus "Menggantikan ADR-NNNN"), sama seperti aturan versi beku TownHall.
6. **DB per service.** Dilarang menyentuh basis data divisi lain. Basis data logis hanya `lumbung`; Titeny hanya membaca (read-only) ke DB sumber lewat kontrak event, bukan lewat akses langsung dari repo ini.
7. **Angka dari dokumen.** Angka bisnis (tarif, ambang, batas) hanya dari dokumen TownHall; jangan mengarang atau menghitung ulang di tempat yang dilarang.
8. **Auth.** `docs/auth.md` adalah sumber kebenaran auth; perubahan auth = ADR baru + perbarui `docs/auth.md` dan `docs/api/openapi.yaml` dalam PR yang sama.
9. **Kontrak.** `docs/api/openapi.yaml` harus selalu cocok dengan `FSD/30-kontrak.md`; handler tanpa entri OpenAPI tidak boleh digabung.

## Definisi selesai

Dari [townhall-roadmap.md §6](https://github.com/Coding-Skuy/ChefGenie-TownHall/blob/main/townhall-roadmap.md), berlaku per milestone:

- [ ] Dokumen versi lengkap (BRD/FRD/FSD/PRD/SRS) + `SNAPSHOT-ROADMAP.md` di Lumbung-TownHall.
- [ ] QA gate berbasis SRS lulus (TDD merah-hijau); rilis diblokir bila gagal.
- [ ] Kontrak API terbit; audit keamanan bersih (JWT audiens `lumbung` + DB per service).
- [ ] Bukti metrik lintas-divisi (KONTEKS §7) tercatat.
- [ ] CHANGELOG versi diperbarui; dokumen beku tidak diedit.
