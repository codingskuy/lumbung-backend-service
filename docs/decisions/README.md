# Architecture Decision Records

Keputusan arsitektur repo ini dicatat sebagai ADR.

## Cara menambah ADR

1. Nomor berurutan: `ADR-NNNN-<judul-kebab>.md` (berikutnya = nomor terakhir + 1, empat digit).
2. Satu keputusan per file. Format: Status (Diusulkan | Diterima | Menggantikan ADR-NNNN), Tanggal, Konteks, Keputusan, Konsekuensi.
3. Immutable: ADR yang sudah Diterima tidak diedit. Untuk mengubah keputusan, tulis ADR baru yang menyatakan "Menggantikan ADR-NNNN" dan catat di CHANGELOG repo.
4. Perubahan kontrak API/event wajib punya ADR (bump versi kontrak di dalamnya).
5. Requirement bisnis tidak diputuskan di sini; ia berasal dari Lumbung-TownHall (`platform/versions/`).

## Daftar

- [ADR-0001 — Fondasi teknis](./ADR-0001-fondasi-teknis.md)
- [ADR-0002 — Implementasi autentikasi LMB-BE-003](./ADR-0002-implementasi-auth.md)
- [ADR-0003 — Skema lot, sortir, dan keranjang (LMB-BE-004)](./ADR-0003-skema-lot-sortir-keranjang.md)
- [ADR-0004 — Trip, surat jalan, endpoint sopir, dan sinkron offline (LMB-BE-005)](./ADR-0004-trip-sopir-sinkron.md)
- [ADR-0005 — Fee per kg, invoice mingguan, dan pembayaran petani H+1 (LMB-BE-006)](./ADR-0005-fee-invoice-pembayaran.md)
- [ADR-0006 — Keputusan pemilik produk: Grade B 85%, hak tulis admin minimum, gram integer](./ADR-0006-keputusan-pemilik-produk.md)
- [ADR-0009 — Endpoint tulis tarif fee (LMB-BE-008)](./ADR-0009-tarif-fee-admin.md)
