# ADR-0004 — Trip, surat jalan, endpoint sopir, dan sinkron offline (LMB-BE-005)

- Status: Diterima
- Tanggal: 2026-10-21

## Konteks

Kontrak v1.0.0 (FSD/30, FSD/10, `produk/30-kontrak-api-KMP-mobile.md`) mendefinisikan `POST /trip`, `POST /trip/{id}/surat-jalan`, lima endpoint sopir, dan `POST /sinkron/batch`. Kontrak tidak menjelaskan dari mana titik tujuan, sopir yang ditugaskan, dan tanggal trip berasal, padahal `GET /trip-hari-ini`, `POST /titik/{id}/tiba` dan fee per pesanan (LMB-BE-006) membutuhkannya. Dokumen Pasaree/Pedaree dan entitas Pesanan belum menjadi bagian tiket ini.

## Keputusan

1. **Tambahan opsional pada `BuatTripRequest`** (tidak merusak klien lama): `tanggal` (bawaan hari ini WIB), `sopir` (akun sopir aktif), dan `titik[]` (`id`, `zona`, `pesanan?`, `keranjang?`). Tanpa `sopir`, trip tidak tampil di `GET /trip-hari-ini`. Skema `Trip` mendapat `tanggal` dan `sopir`. **Perlu keputusan Tech Lead**: bila TownHall menetapkan sumber titik/sopir lain (mis. dari pesanan), tambahan ini diganti lewat ADR baru.
2. **Kode trip** `TRIP-RUTE-NNN` dari penghitung permanen per rute (`UT`, `SL`, `KT`); pemetaan `SELATAN -> SL` dan `KOTA -> KT` adalah asumsi (kontrak hanya memberi `UT`). Melewati 999 ditolak 409 karena pola `\d{3}` pada kontrak.
3. **Kas jalan** tetap Rp300.000 (FSD/20): nilai lain pada permintaan 422.
4. **Trip memuat keranjang** yang sudah dicatat (`keranjang_isi` aktif, belum di trip lain, dikunci `FOR UPDATE`); lot terkait menjadi `DIMUAT`. Keranjang dapat ditetapkan ke titik (`titik_id`) sehingga bukti terima per titik dapat mengosongkannya; yang tanpa titik dikosongkan saat trip `SELESAI`.
5. **Surat jalan** satu per trip (`UNIQUE trip_id`), nomor `SJ-YYYYMMDD-NNN` dari `urutan_harian` (tanggal WIB). `{id}` dapat berupa kode trip atau UUID klien trip karena klien offline belum mengetahui kode resmi. Kontrak sinkron tidak memuat trip pada rekaman `surat_jalan`, sehingga payload-nya wajib memuat `trip_id`.
6. **Status trip**: `DISUSUN` -> `BERJALAN` (tiba pertama) -> `SELESAI` (semua titik berangkat). Kejadian tiba/berangkat berulang dengan UUID lain menimpa waktu (lapangan menang untuk posisi); berangkat wajib setelah tiba. Urutan kunjungan antar titik tidak dipaksa (offline dapat mengirim tidak berurutan).
7. **Otorisasi sopir**: hanya sopir yang ditugaskan pada trip (403 + audit). Peran diperiksa sebelum badan dibaca (ekstraktor `OperatorGudang`, `PeranSopir`, `AdminGudangAkses`) sehingga peran salah selalu 403, bukan 422 dari validasi badan; ekstraktor ini juga dipakai ulang oleh endpoint LMB-BE-004 dan LMB-BE-007.
8. **Foto**: JPEG utuh (awal `FFD8FF`, akhir `FFD9`), 1 sampai 512.000 byte (500 x 1024; definisi KB mengikuti 1024), disimpan sebagai `BYTEA` di tabel `foto` dengan `simpan_sampai` 1 tahun (penghapusan konten setelah itu belum dikerjakan). `uuid` menjadi `foto_id`. Memerlukan fitur `multipart` pada `axum` (menambah `multer` secara transitif; versi axum tidak berubah). Badan di atas 1 MiB ditolak 413 sebelum dibaca; selebihnya pelanggaran 422. Penyimpanan di basis data dipilih agar transaksional dan sederhana untuk pilot; pindah ke object storage bila volume membesar.
9. **Bukti terima**: minimal satu foto atau tanda tangan, maksimal 4 foto yang sudah diunggah. Bukti baru untuk titik yang sama dengan UUID lain menang (lapangan menang), riwayat disimpan, dan penggantian dicatat sebagai insiden `konflik_sinkron`. Bukti terima mengosongkan keranjang titik itu; lot menjadi `DITERIMA` bila seluruh isi keranjangnya selesai.
10. **Insiden** `INS-YYYYMMDD-NNN` dari `urutan_harian`; konflik sinkron memakai tabel yang sama dengan `jenis = konflik_sinkron` (dasar `GET /admin/insiden`, belum ditugaskan).
11. **Sinkron batch**: 1 sampai 200 rekaman diproses berurutan FIFO dalam satu transaksi dengan savepoint per rekaman; hasil per rekaman `DITERIMA|DUPLIKAT|KONFLIK|DITOLAK`. Payload adalah badan endpoint asal tanpa `uuid` (diisi dari rekaman, bila ada harus sama). `nomor_sementara` (`TMP-` + 8 heksa huruf kecil) dipetakan: lot memakai kolom `lot.nomor_sementara`, surat jalan `surat_jalan.nomor_sementara`, dan `nomor_resmi` dikembalikan. Galat basis data/internal membatalkan seluruh request (aman dikirim ulang). `KONFLIK` hanya untuk bentrokan data gudang (berat/grade; gudang menang, dicatat); validasi gagal `DITOLAK`.
12. **Kunci offline 3x24 jam**: rekaman `surat_jalan` dengan `dibuat_pada` lebih dari 72 jam dari waktu terima `DITOLAK`; lot, sortir, keranjang, bukti terima dan insiden tetap diterima. Dihitung dari umur rekaman, bukan dari sinyal "perangkat offline" yang tidak ada di kontrak.
13. **Login sopir** menyertakan `trip_hari_ini` (kode trip ditugaskan hari ini) sesuai contoh `produk/50`.
14. **Skema**: migrasi `0005_trip_sinkron.sql` membuang `trip` templat 0001 (belum dipakai, tanpa data) lalu membuat `trip`, `trip_titik`, `surat_jalan`, `titik_event`, `foto`, `bukti_terima`, `insiden`, `penghitung`, dan menambah `trip_id`/`titik_id` pada `keranjang_isi`.

## Konsekuensi

- LMB-BE-006 menghitung fee dari `trip_titik.zona`, `trip_titik.pesanan`, dan berat keranjang per titik.
- Notifikasi ke pihak yang kalah pada konflik belum ada; respons `KONFLIK`/`DITOLAK` membawa `pesan` dan konflik masuk log insiden.
- Retensi foto 1 tahun belum otomatis; endpoint pembuat/penugasan trip dari pesanan menunggu definisi Pesanan.
