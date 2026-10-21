# Uji kontrak (`contract/`)

Sumber kebenaran kontrak: [`docs/api/openapi.yaml`](../docs/api/openapi.yaml). Skrip di sini menguji server nyata
terhadap kontrak itu. Telemetri alat uji dimatikan dan tidak ada data keluar dari mesin.

## Schemathesis (LMB-BE-TEST-SCHEMATHESIS)

Prasyarat: Postgres lokal, `psql`, `uvx` (uv), `openssl`, `curl`, `python3`.

```bash
createdb lumbung_uji
export DATABASE_URL=postgres://$USER@localhost:5432/lumbung_uji
openssl genpkey -algorithm ed25519 -out /tmp/priv.pem && openssl pkey -in /tmp/priv.pem -pubout -out /tmp/pub.pem
JWT_ED25519_PRIVATE="$(cat /tmp/priv.pem)" JWT_ED25519_PUBLIC="$(cat /tmp/pub.pem)" \
  DEVICE_API_KEY_GUDANG=kunci-gudang-lokal SERVICE_API_KEY=kunci-layanan-lokal \
  LISTEN_ADDR=127.0.0.1:8080 cargo run &           # migrasi berjalan otomatis saat boot

BASE_URL=http://127.0.0.1:8080/v1 DEVICE_KEY=kunci-gudang-lokal SERVICE_KEY=kunci-layanan-lokal \
  MAX_EXAMPLES=50 contract/schemathesis.sh
```

`schemathesis.sh` menyemai akun uji (`seed.py`, PIN 123456; hanya untuk DB uji), masuk sebagai tiap peran, lalu
menjalankan enam pass per peran/jalur (`publik`, `gudang`, `sopir`, `petani`, `admin-layanan`, `admin-keuangan`)
dan `gudang-keluar` terpisah (operasi itu mencabut token shift). Laporan JUnit di `contract/laporan/`.
CI manual: workflow `Schemathesis (manual)` (`workflow_dispatch`).

Pengecualian yang disengaja (ADR-0007): check `positive_data_acceptance` (aturan domain 422 atas data yang lolos
skema, mis. berat gram vs toleransi, hari Senin, foto belum diunggah) dan tiga operasi yang menjawab 501
terdokumentasi (`kontrakSaya`, `rencanaSetoran`, `adminMitraStatus`).

## Pact, verifikasi provider (LMB-BE-TEST-PACT)

Lihat `contract/pact/README.md` dan ADR-0008.
