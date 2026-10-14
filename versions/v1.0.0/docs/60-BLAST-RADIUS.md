> Versi: v1.0.0 | Status: disetujui | Menggantikan: -
> Sumber: Lumbung-TownHall `platform/versions/v1.0.0/` pada commit `673a467`; [townhall-roadmap.md §2](https://github.com/Coding-Skuy/ChefGenie-TownHall/blob/main/townhall-roadmap.md); [KONTEKS §8/§9](https://github.com/Coding-Skuy/ChefGenie-TownHall/blob/main/KONTEKS-LINTAS-DIVISI.md)

# 60 — Blast Radius lumbung-backend-service

Batas context v1.0.0 Lumbung (townhall-roadmap §2): 3 komoditas pilot, fee Rp1.500/kg dekat dan Rp2.500/kg jauh, radius 25 km; point of concern titik impas 18.000 kg/bulan dan susut <5%.

## Boleh disentuh

- Skema, migrasi, dan data DB logis `lumbung`.
- Seluruh endpoint kontrak FSD/30: sopir (5), petani (4), gudang (7), web publik (2), web admin (4 baca + 3 tulis), auth.
- Perhitungan fee per kg, invoice mingguan (Senin, jatuh tempo 7 hari, denda 1%/minggu maks 4%), pembayaran petani H+1 (denda 0,5%/hari maks 5%), papan harga.
- Serving model dari `lumbung-ai-models` (rute internal berkunci service key).

## Dilarang

- Menyentuh DB logis divisi lain (`pasaree`, `titipo`, `pedaree`, `pawonee`, `titeny`).
- Menambah `event_type` di luar registry KONTEKS §3 tanpa bump versi + 30 hari dual-read.
- Menambah komoditas keempat atau melayani di luar radius 25 km (non-goal PRD/30).
- Menerima berat dalam satuan selain gram integer dari klien web (web mengonversi kg desimal ke gram sebelum memanggil).
- Mengeluarkan kredensial perangkat/PIN dalam log atau respons.
- Mengubah kontrak tanpa ADR + bump versi OpenAPI.

## Kontrak diproduksi

- API REST `/v1` (OpenAPI 3.1) untuk app-kmp, web, data-pipeline, ai-models.
- Event domain FSD/30: `lot.disortir`, `trip.dimuat`, `titik.tiba`, `bukti.diterima`, `fee.ditagih`, `petani.dibayar`, `konflik.dicatat` (UUID, waktu, versi); dipetakan oleh `lumbung-data-pipeline` ke registry Titeny (`source: lumbung`).
- Auth: JWT akses/refresh, token shift gudang 12 jam, `X-Device-Key`, `X-Service-Key`.

## Kontrak dikonsumsi

- Data harga referensi dan ingest dari `lumbung-data-pipeline` (service key).
- Prediksi dari `lumbung-ai-models` (serving Rust).
- Tidak ada konsumsi dari DB atau API divisi lain pada v1.0.0.

## Aturan perubahan

Perubahan di luar batas ini = PR requirement di Lumbung-TownHall lebih dulu, lalu ADR baru di repo ini. Perubahan angka lintas-divisi wajib memperbarui KONTEKS §terkait + dokumen divisi + CHANGELOG versi dalam PR yang sama (KONTEKS §9).
