# Verifikasi provider Pact (LMB-BE-TEST-PACT)

Backend adalah *provider*. `verifikasi.mjs` (pact-js 17.1.4, pustaka native Pact Rust) memutar ulang tiap
interaksi dari berkas di `pacts/*.json` terhadap server nyata dan menyiapkan datanya lewat *state handler*.
Sumber kebenaran kontrak tetap `docs/api/openapi.yaml`; pact hanya memastikan apa yang benar-benar dipakai konsumen.

## Berkas pact

| Berkas | Konsumen | Sumber |
|---|---|---|
| `pacts/lumbung-web-lumbung-backend-service.json` | lumbung-web | lumbung-web PR #16 (LMB-WEB-TEST-PACT), disalin apa adanya |
| (belum ada) | lumbung-app-kmp | menunggu LMB-KMP-TEST-PACT (app-kmp #8); lihat ADR-0008 |

Berkas baru cukup ditaruh di `pacts/`; state yang belum dikenal harus ditambahkan ke `verifikasi.mjs`.

## Menjalankan (lokal, DB UJI)

State handler menghapus/mengubah data: pakai basis data uji, jangan produksi.

```bash
# server lokal berjalan (lihat contract/README.md), lalu:
cd contract/pact && npm install --ignore-scripts      # --ignore-scripts mematikan analitik postinstall (scarf)
DATABASE_URL=postgres://$USER@localhost:5432/lumbung_uji SERVICE_KEY=kunci-layanan-lokal \
  PROVIDER_URL=http://127.0.0.1:8080 node verifikasi.mjs
```

Butuh Node 20+, `psql`. Keluaran memuat tiap interaksi (status, header, badan) dan `LULUS`/`GAGAL`; exit 1 bila gagal.

## State provider

| State | Penyiapan |
|---|---|
| `papan harga minggu berjalan sudah terbit` | `POST /v1/admin/papan-harga` (kunci layanan), Senin minggu berjalan WIB |
| `papan harga minggu berjalan belum terbit` | `DELETE FROM papan_harga` |
| `profil Lumbung tersedia` | `UPDATE profil_publik` (deskripsi, kontak kemitraan) |
| `basis data tidak tersedia` | `ALTER TABLE profil_publik RENAME` sementara (kueri gagal, 500), dipulihkan di teardown |

## CI manual

Workflow `Pact provider (manual)` (`workflow_dispatch`): postgres + server + `verifikasi.mjs`. Ditulis, belum dijalankan di Actions.
Broker Pact tidak dipakai (berkas di-commit).
