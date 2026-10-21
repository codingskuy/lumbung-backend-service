# ADR-0006 — Keputusan pemilik produk: Grade B 85%, hak tulis admin minimum, gram integer, `.env.example`

- Status: Diterima
- Tanggal: 2026-10-21
- Menggantikan sebagian: ADR-0005 butir 7 (rumus nominal Grade B) dan butir 5 (otorisasi `lunas`)

## Konteks

Pemilik produk menetapkan lima keputusan yang menutup ambiguitas dan temuan di ADR-0003, ADR-0004, dan ADR-0005 (diteruskan ke repo ini sebagai keputusan yang sudah disetujui). ADR lama tidak diedit; ADR ini menggantikan butir yang bertentangan.

## Keputusan

1. **Status pembayaran** tetap `MENUNGGU`, `DIBAYAR`, `TERLAMBAT` (enum `Pembayaran.status` pada kontrak; tidak ada penyimpangan di implementasi).
2. **Grade B dibayar 85 persen harga papan** (BRD: A 100%, B 85%, C ditolak). Harga papan yang menjadi acuan adalah harga papan komoditas itu (harga Grade A). Nominal = `A_g x harga / 1000 + B_g x harga x 85 / 100 / 1000`, dibulatkan setengah naik per grade ke rupiah. Menggantikan ADR-0005 butir 7 yang membayar Grade B sebesar harga papan B. Contoh L-8812 (cabai, papan 31.500): `100.000 g -> Rp3.150.000` dan `14.000 g -> Rp374.850`, total Rp3.524.850 (sebelumnya Rp3.521.000). Harga papan B pada papan harga (publik/petani) tetap terbit sebagai informasi dan tidak dipakai menghitung pembayaran. Fee logistik tidak terpengaruh (berbasis berat dan zona, tidak berbasis grade).
3. **Berat gram integer di mana pun.** Skema akhir tidak punya kolom `NUMERIC`/desimal untuk berat atau uang dan tidak punya kolom berbasis kg (kecuali `tarif_per_kg`, rupiah per kg). Migrasi `0001_init.sql` memang mendefinisikan tabel templat `NUMERIC(…)` kg, tetapi migrasi immutable (ADR-0001, AGENTS.md); tabel itu dibuang oleh `0004` dan `0005` sebelum dipakai kode mana pun. Dijaga oleh uji `tests/skema.rs` yang gagal bila ada kolom desimal atau `*_kg` di skema akhir. Hanya `titik_event.lat/lng` memakai `double precision` (koordinat, bukan berat).
4. **`.env.example`** mencantumkan lima rahasia tanpa nilai: `DATABASE_URL`, `JWT_ED25519_PRIVATE`, `JWT_ED25519_PUBLIC`, `DEVICE_API_KEY_GUDANG`, `SERVICE_API_KEY` (sisi web menyimpan nilai yang sama sebagai `LUMBUNG_API_KEY`). `.gitignore` mengecualikan `.env.example` dari aturan `.env.*`; nilai nyata tidak pernah di-commit.
5. **Hak tulis admin minimum.** Admin gudang hanya menulis papan harga (`POST /admin/papan-harga`); alat gudang memakai `X-Device-Key` untuk 7 endpoint desktop (lot, sortir, keranjang batch, trip, surat jalan, stok-lot, sinkron). Tarif fee tetap konstanta kode dari dokumen (tidak ada endpoint tulis tarif). Penulisan keuangan lain hanya bila ada tiketnya: `POST /admin/invoice/{id}/lunas` (tiket LMB-BE-006) kini hanya `admin_keuangan` atau `X-Service-Key` (web); `admin_gudang` mendapat 403 (tetap boleh membaca daftar invoice bersama `admin_keuangan`). Status mitra, rekap, dan penulisan admin lain tidak diimplementasi sampai ada tiketnya.

## Konsekuensi

- Klien yang membaca `pembayaran-saya` melihat nominal Grade B lebih tinggi dibanding perhitungan lama (85% dari harga A, bukan harga papan B yang dibulatkan ke bawah Rp500).
- Perubahan peran pada `lunas` memerlukan akun `admin_keuangan` atau kunci layanan untuk operasi itu.
- Bila BRD berubah (persentase atau Grade C), ubah `domain::tarif` dan ADR baru.
