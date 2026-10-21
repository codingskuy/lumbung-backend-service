# ADR-0003 — Skema lot, sortir, dan keranjang (LMB-BE-004)

- Status: Diterima
- Tanggal: 2026-10-21

## Konteks

ADR-0001 menyatakan skema templat `0001_init.sql` (berat `NUMERIC` kg, ID `uuid`) harus diselaraskan dengan kontrak v1.0.0 (berat gram integer, kode `L-NNNN`) lewat migrasi baru di tiket LMB-BE-004. FSD/20 mendefinisikan entitas Panen, Lot, Timbangan, Keranjang; FSD/30 dan tiket mendefinisikan endpoint lot, sortir, keranjang batch, dan stok-lot.

## Keputusan

1. **Migrasi `0004_lot_sortir.sql` membuang lalu membuat ulang** `lot`, `timbangan`, `fee`, `panen` templat. Tabel itu belum dipakai kode mana pun dan tidak berisi data (v1.0.0 belum rilis); mengubahnya di tempat tidak mungkin karena tipe kolom dan FK berubah. `trip` templat tidak disentuh di sini (diselaraskan di LMB-BE-005) dan `fee` dibuat ulang di LMB-BE-006. Migrasi `0001` tidak diedit.
2. **Panen digabung ke lot.** Kontrak `POST /lot` membawa `kode_petani`, `komoditas`, `tanggal`, `berat_kotor_g` sekaligus; tabel `panen` terpisah tidak punya endpoint dan dibuang. Entitas ini dapat dihidupkan lagi bila TownHall mendefinisikan alur panen sendiri.
3. **Petani.** Tabel `petani` (`PTN-NNN`, `AKTIF|NONAKTIF`) dibuat minimal agar lot punya FK; `kode` sama dengan akun peran petani di `pengguna` (dipakai LMB-BE-006 untuk memfilter per petani). Tidak ada endpoint pembuat petani (di luar tiket; `GET /admin/mitra` ada di kontrak tetapi belum ditugaskan). Lot hanya dapat dibuat untuk petani `AKTIF`.
4. **Kode lot** `L-NNNN` dari `SEQUENCE lot_kode_seq` (global, empat digit minimum, tidak mengulang). Nomor sementara `TMP-{uuid8}` disimpan di `lot.nomor_sementara` (unik) supaya path `{id}` dapat berupa keduanya; pemetaan otomatis terjadi di LMB-BE-005.
5. **Toleransi sortir** dihitung dengan aritmetika integer: `|A+B+C - kotor| * 100 <= kotor * 2`. Batas tepat 2 persen diterima. Total harus dalam toleransi, bukan sama persis; selisih tidak dikoreksi otomatis.
6. **Timbangan:** `KOTOR` ditulis saat lot dibuat, `BERSIH` = total A+B+C saat sortir (susut = kotor - bersih). Sortir hanya sekali per lot (`UNIQUE lot_id`); sortir ulang dengan UUID berbeda `409` ("data gudang lebih baru").
7. **Grade C** dicatat di `alih_grade_c` saat sortir dan tidak dihitung dalam stok rantai dingin. Memasukkan Grade C ke keranjang ditolak 422 dan dicatat di audit. Tujuan alih (olahan/pembuangan) tidak didefinisikan dokumen sehingga hanya `alasan` standar yang disimpan.
8. **Keranjang berkode permanen** `KRJ-001..120`: tabel `keranjang_isi` dengan indeks unik parsial `(kode) WHERE selesai_at IS NULL`; satu kode hanya satu isi aktif (409 bila masih terisi), dan `selesai_at` diisi oleh alur trip/bukti terima (LMB-BE-005). Batas 120 berasal dari komentar kontrak ("KRJ-001 sampai KRJ-120").
9. **Stok** = hasil sortir per grade dikurangi isi keranjang. Batch mengunci baris lot (`FOR UPDATE`, berurutan menurut kode) sehingga dua batch bersamaan tidak sama-sama melewati stok; batch bersifat atomik.
10. **Idempotensi:** `uuid_klien` unik di `lot`, `sortir`, `keranjang_batch`; kirim ulang memberi 200 `{id, duplikat: true}` sebelum validasi status, sehingga pengiriman ulang setelah keadaan berubah tetap aman.
11. **Otorisasi:** keempat endpoint memakai `Gudang` (bearer + `X-Device-Key`, sesuai OpenAPI) dan peran `admin_gudang`; peran lain 403 dan dicatat. Service key tidak diterima.

## Konsekuensi

- Skema baru butuh seed petani dan akun operator untuk dipakai; belum ada endpoint admin untuk keduanya.
- LMB-BE-005 menambah `selesai_at`, status `DIMUAT`/`DITERIMA`, dan pemetaan `TMP-*`; LMB-BE-006 memakai berat bersih per grade dari `lot`.
- Skema permintaan bersifat ketat (`additionalProperties: false`): bidang asing 422, sehingga klien tidak dapat menyelundupkan angka hitungan server.
