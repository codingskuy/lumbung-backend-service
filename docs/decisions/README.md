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
