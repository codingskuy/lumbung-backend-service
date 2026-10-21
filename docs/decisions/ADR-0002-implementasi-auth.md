# ADR-0002 — Implementasi autentikasi LMB-BE-003

- Status: Diterima
- Tanggal: 2026-10-21

## Konteks

ADR-0001 mengunci JWT ed25519 (akses 15 menit, refresh 7 hari dengan reuse-detection), `X-Device-Key` (rotasi 90 hari, tenggang 7 hari), dan `X-Service-Key`. Tiket LMB-BE-003 mewujudkannya. Beberapa hal tidak ditetapkan dokumen dan diputuskan di sini; tiap butir yang bersifat kebijakan ditandai untuk ditinjau Tech Lead.

## Keputusan

1. **Kunci JWT.** Dua variabel: `JWT_ED25519_PRIVATE` (PEM PKCS#8) dan `JWT_ED25519_PUBLIC` (PEM SPKI). Menurunkan kunci publik dari privat butuh dependensi baru (`ring`/`ed25519-dalek`) sehingga dihindari; `jsonwebtoken` `9.3.1` yang sudah terkunci cukup. Fallback HS256 yang disebut di komentar `config.rs` lama dihapus.
2. **Refresh token.** Format opak `<uuid>.<rahasia 256 bit>`; rahasia dibangkitkan dengan `OsRng` (sudah tersedia lewat `argon2`), di basis data hanya `sha256` (fungsi `digest` pgcrypto, ekstensi sudah ada). Rotasi tiap pemakaian dalam satu `family_id`. Pemakaian ulang token terpakai atau dicabut mencabut **semua** keluarga pengguna tersebut (sesuai tiket), dan pencabutan dikomit walau responsnya 401.
3. **Keluar.** `POST /auth/keluar` menghapus keluarga refresh yang tertanam di klaim `fam` token akses (sesi perangkat lain tidak ikut). Token akses tetap berlaku sampai habis (15 menit) kecuali akun dinonaktifkan: ekstraktor memeriksa `pengguna.aktif` pada tiap permintaan.
4. **Kunci PIN.** Tiga salah PIN berturut-turut mengunci akun 15 menit; selama terkunci jawaban 429 (kontrak memuat 429 pada login) walau PIN benar. PIN benar mereset hitungan. Akun tidak ada dan PIN salah sama-sama 401, dengan verifikasi argon2 palsu agar waktu respons setara.
5. **Kunci perangkat.** Tabel `kunci_perangkat` (hash saja). Berlaku 90 hari sejak dibuat; rotasi mendaftarkan kunci baru dan memendekkan kunci lama pada label yang sama menjadi paling lama 7 hari. Nilai `DEVICE_API_KEY_GUDANG` didaftarkan otomatis saat boot bila belum dikenal. Pencabutan seketika disediakan sebagai fungsi (`kunci::cabut`); endpoint pencabutan belum ada di kontrak. **Interpretasi kebijakan:** "tenggang 7 hari" dihitung setelah rotasi, bukan setelah hari ke-90.
6. **Kunci layanan.** Perbandingan waktu konstan terhadap `SERVICE_API_KEY`; hanya sah pada rute `/admin/*` dan rute layanan, tidak pernah pada `/auth/*`, data pengguna, atau endpoint gudang. Aktor audit untuk panggilan layanan dicatat `layanan-web` karena kontrak belum membawa nama admin di sisi web (lihat Risiko).
7. **Login shift gudang.** `POST /gudang/masuk` menuntut `X-Device-Key` sah + PIN operator berperan `admin_gudang`; token shift 12 jam dicatat di `sesi_shift` agar `POST /gudang/keluar` dapat mencabutnya.
8. **Audit.** `audit_log` mencatat login, gagal masuk, penguncian, pemakaian ulang refresh, dan penolakan 403 (produk/50 butir 5); `AUD-YYYYMMDD-NNN` dibangkitkan dari `urutan_harian` (upsert atomik, tanggal WIB). Pemberitahuan ke Kepala Agregasi saat terkunci baru berupa entri audit `akun_terkunci`; pengiriman notifikasinya di luar tiket.
9. **Awalan rute.** Seluruh API di bawah `/v1`; server lokal OpenAPI diubah menjadi `http://localhost:8080/v1` agar identik dengan produksi. `/healthz` infra tetap di akar.
10. **Dependensi.** Hanya dev-dependency baru `tower` (`util`, uji `oneshot`) dan `ring` (membangkitkan kunci uji sementara; keduanya sudah ada di `Cargo.lock` lewat dependensi transitif) serta fitur `json` pada `sqlx` untuk kolom `jsonb`. Tidak ada kunci rahasia di repo.

## Konsekuensi

- Ketiga kredensial punya jalur terpisah (bearer, `X-Device-Key`, `X-Service-Key`); handler memilih ekstraktor `Pengguna`, `Gudang`, `PerangkatGudang`, `Layanan`, atau `AkunAdmin`, sehingga hak minimal diperiksa oleh tipe.
- Tidak ada endpoint pembuatan akun, penggantian PIN (90 hari), atau pencabutan perangkat; akun dibuat lewat SQL atau alat admin di tiket berikutnya.
- Batas laju 60 request per menit per perangkat belum diterapkan (di luar LMB-BE-003).

## Risiko dan hal yang perlu keputusan manusia

- Token perangkat mobile 30 hari di `produk/50-autentikasi-perangkat.md` bertentangan dengan JWT akses 15 menit di FSD/30 dan `docs/auth.md`; implementasi mengikuti 15 menit + refresh 7 hari (standar repo).
- Siapa nama admin yang tercatat bila web SvelteKit memakai `X-Service-Key` untuk tindakan admin: perlu header aktor di kontrak (usulan `X-Aktor`) atau sesi peran diteruskan sebagai bearer.
