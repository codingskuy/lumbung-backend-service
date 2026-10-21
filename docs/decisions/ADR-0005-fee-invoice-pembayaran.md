# ADR-0005 — Fee per kg, invoice mingguan, dan pembayaran petani H+1 (LMB-BE-006)

- Status: Diterima
- Tanggal: 2026-10-21

## Konteks

BRD/30 (BR-301 sampai BR-306) dan FRD (FR-201..FR-204) menetapkan tarif fee per kg, invoice mingguan per pembeli, denda, dan pembayaran petani H+1. Dokumen tidak mendefinisikan entitas Pesanan/Pembeli di repo ini, tidak menyebut cara pembulatan, tidak menjelaskan arti "minggu berjalan" untuk harga bayar, dan kontrak tidak memuat endpoint untuk menandai pembayaran petani sudah dibayar. Keputusan di bawah ditandai bila bersifat kebijakan dan perlu persetujuan Tech Lead.

## Keputusan

1. **Fee** dihitung server saat surat jalan terbit, per pesanan: berat gram keranjang yang ditetapkan ke titik x tarif zona per kg (DEKAT Rp1.500, JAUH Rp2.500) / 1.000, integer rupiah. Contoh BR-301: 170.000 g zona dekat = Rp255.000. **Pembulatan setengah naik** ke rupiah terdekat untuk pecahan kg (asumsi; dokumen tidak menyebut). Tarif hanya di `domain::tarif` (angka dari dokumen).
2. **Sumber pesanan dan pembeli.** Entitas Pesanan belum ada di repo ini, sehingga titik trip membawa `pesanan` (`PSO-YYYYMMDD-NNN`) dan `pembeli` opsional (tambahan opsional pada `BuatTripRequest.titik[]`, melanjutkan ADR-0004). Fee hanya dibuat untuk titik berpesanan yang punya keranjang ditetapkan; fee tanpa `pembeli` tersimpan tetapi tidak masuk invoice. **Perlu keputusan Tech Lead** bila Pesanan akan menjadi entitas sendiri.
3. **Invoice** per pembeli per minggu ISO surat jalan (`minggu` pada kontrak = minggu surat jalan yang ditagih). Terbit Senin sesudah minggu itu berakhir, jatuh tempo 7 hari kemudian, kode `INV-YYYYMMDD-NNN` (tanggal terbit). Tanpa penjadwal eksternal: generasi idempoten dijalankan malas saat `GET /admin/invoice` dibaca dan oleh tugas latar per jam di `main.rs` (`terbitkan_tertunda`); dilindungi `pg_advisory_xact_lock` per minggu agar penomoran tidak berbalapan. Minggu yang belum berakhir menghasilkan daftar kosong.
4. **Denda invoice** 1 persen per minggu keterlambatan dihitung per minggu atau bagian minggu (1 hari telat = 1 persen, 8 hari = 2 persen), maksimal 4 persen, dari nominal tanpa denda, dibulatkan setengah naik. Dibekukan saat lunas (`POST /admin/invoice/{id}/lunas`). Status `LEWAT_JATUH_TEMPO` diturunkan dari tanggal, bukan disimpan. Interpretasi "per minggu" (bagian minggu dihitung penuh) adalah asumsi.
5. **Otorisasi invoice**: `admin_keuangan` dan `admin_gudang` (PRD: admin gudang menagih fee mingguan, admin keuangan melihat invoice) serta `X-Service-Key`; peran lain 403. Lunas ulang 409, kode tak ada 404, audit `audit_id` via `audit::catat_beraudit`.
6. **Pembayaran petani** dibuat saat sortir dicatat (berat bersih per grade diketahui; juga lewat sinkron karena memakai `sortir_tx`), jatuh tempo H+1 menurut tanggal WIB. Definisi "lot diterima" untuk H+1 adalah saat sortir selesai di gudang (bukan status lot `DITERIMA`, yang berarti sampai di pembeli), agar petani tidak menunggu pengiriman. **Perlu konfirmasi.**
7. **Nominal** = berat bersih Grade A x harga papan A + berat bersih Grade B x harga papan B (per komoditas), papan yang berlaku pada tanggal sortir, dihitung saat dibaca dan dibekukan saat dibayar. Grade C tidak dibayar. **Ambiguitas dokumen:** tiket menulis "Grade A 100%, B 85%"; harga papan Grade B sudah dibangun dari 85 persen harga A (ADR-0003/papan), sehingga Grade B dibayar sebesar harga papan B, bukan 85 persen lagi dari harga papan B. Lot yang papan minggunya belum terbit belum tampil di `GET /pembayaran-saya` (nominal belum dapat dihitung) dan tampil setelah papan terbit.
8. **Denda pembayaran** Lumbung 0,5 persen per hari keterlambatan, maksimal 5 persen (10 hari), otomatis: dihitung dari tanggal saat dibaca; status `TERLAMBAT` setelah jatuh tempo.
9. **Menandai dibayar**: kontrak tidak punya endpoint; disediakan fungsi `pembayaran::tandai_dibayar` (membekukan nominal dan denda) untuk alur keuangan/ops sampai kontrak mendefinisikannya. **Celah kontrak** dilaporkan ke Tech Lead.
10. **Filter petani** di server: `GET /pembayaran-saya` hanya peran `petani`, kunci filter adalah akun sesi (akun peran = kode petani `PTN-NNN`), parameter klien diabaikan.
11. **Skema** `0006_fee_invoice_pembayaran.sql`: `fee` (dibuat ulang setelah dibuang di 0004), `invoice`, `pembayaran_petani`, kolom `trip_titik.pembeli`. `fee.status_cetak` tersedia untuk hak `X-Device-Key` (`fee.status_cetak`), endpoint penandaannya belum ada di kontrak.

## Konsekuensi

- Tidak ada pembayaran uang nyata di sistem ini; hanya pencatatan tagihan, status, dan denda.
- Contoh dokumen "papan 6 sampai 12 Okt 2026" disebut berlaku Senin sampai Minggu, padahal 6 Oktober 2026 adalah Selasa (Senin = 5 Oktober); implementasi memakai Senin-Minggu sesuai aturan, sehingga contoh tanggal pada FSD/OpenAPI tidak cocok dengan kalender (temuan dokumen).
- Invoice terbit tertunda bila layanan mati lebih dari satu minggu; tugas latar dan pembacaan malas menyusulnya dengan tanggal terbit minggu yang benar.
