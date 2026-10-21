# ADR-0009 — Endpoint tulis tarif fee (LMB-BE-008)

- Status: Diterima
- Tanggal: 2026-10-22

## Konteks

Keputusan pemilik produk (ADR-0006): admin hanya menulis papan harga dan tarif fee. Tarif fee sebelumnya konstanta kode (Rp1.500 dekat, Rp2.500 jauh, BR-301). BR-306: penyesuaian tarif maksimal satu kali per semester, maksimal 10 persen, pemberitahuan 30 hari. Konsumen web (LMB-WEB-005, issue web #18) membutuhkan kontrak yang jelas.

## Keputusan

1. **Kontrak**: `POST /admin/tarif-fee` `{uuid, berlaku_mulai, dekat, jauh}` (rupiah per kg, integer lebih dari 0) menjawab `201 {audit_id, duplikat:false}` atau `200 {audit_id, duplikat:true}`; `GET /admin/tarif-fee` menjawab `{berlaku: {dekat, jauh}, riwayat: [{berlaku_mulai, dekat, jauh, diterbitkan_oleh, audit_id|null}]}`. Ada di `docs/api/openapi.yaml` (`adminTerbitTarifFee`, `adminTarifFee`).
2. **Otorisasi**: tulis hanya `admin_gudang` atau `X-Service-Key` (web), sama dengan papan harga; peran lain 403 dan dicatat. Baca untuk admin mana pun atau kunci layanan. Perubahan beraudit (`AUD-YYYYMMDD-NNN`, aksi `tarif_fee_terbit`).
3. **Aturan BR-306 ditegakkan server**: `berlaku_mulai` minimal hari ini WIB + 30 hari (422); tiap zona berubah paling banyak 10 persen dari tarif yang berlaku pada `berlaku_mulai` (aritmetika integer, 422); satu perubahan per semester kalender (Jan-Jun, Jul-Des, dihitung dari `berlaku_mulai`) dan satu tarif per tanggal (409). Penulisan diserialkan (`pg_advisory_xact_lock`). Interpretasi semester kalender dan "10 persen dari tarif pada hari berlaku" adalah asumsi (dokumen tidak merinci).
4. **Pemakaian**: tarif untuk suatu tanggal adalah baris `tarif_fee` dengan `berlaku_mulai` terbesar yang tidak melewati tanggal itu (baris awal 2000-01-01 = BR-301). Fee surat jalan memakai tarif pada tanggal surat jalan; papan harga menyimpan snapshot tarif pada `berlaku_mulai` papan. Konstanta kode menjadi nilai awal saja. Batasan skema `fee.tarif_per_kg IN (1500, 2500)` dan `const` pada skema OpenAPI `PapanHarga.fee` dilonggarkan menjadi bilangan bulat positif (bentuk respons tidak berubah).
5. **Berat gram integer** tetap; fee tetap `berat_g x tarif / 1000` dibulatkan setengah naik.

## Catatan terbuka (bukan perubahan kode)

Keseragaman pembulatan Grade B dengan lumbung-ai-models (PR #5): ai-models menghitung B = 85 persen papan dibulatkan ke bawah kelipatan Rp500 (31.500 -> B 26.500, 28.000 -> 23.500, 6.000 -> 5.000), sedangkan pembayaran petani di backend (ADR-0006) memakai 85 persen harga papan dibulatkan setengah naik ke rupiah per grade (contoh L-8812: Rp3.524.850 dibanding Rp3.521.000 bila memakai harga papan B). Kode TIDAK diubah sampai pemilik produk memutuskan.

## Konsekuensi

- Web dapat membuat form tarif dari kontrak ini dan menampilkan `audit_id`.
- Fee yang sudah tercatat tidak berubah saat tarif baru dijadwalkan (snapshot per surat jalan).
