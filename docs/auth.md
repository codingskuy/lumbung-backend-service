# Auth Lumbung

## JWT Pengguna

- Token akses: JWT berumur 15 menit, ditandatangani dengan kunci ed25519 (`JWT_ED25519_PRIVATE`).
- Token refresh: umur 7 hari, disimpan hash di basis data.
- Refresh memakai deteksi pakai-ulang (reuse detection): bila refresh token lama dipakai dua kali, seluruh keluarga refresh milik pengguna dicabut dan pengguna wajib masuk ulang.
- Alur keluar: hapus keluarga refresh dari DB; token akses kedaluwarsa sendiri dalam 15 menit.

## Device API Key Gudang

- Header: `X-Device-Key: <DEVICE_API_KEY_GUDANG>`.
- Dipakai perangkat/layanan `gudang-desktop` untuk kirim timbangan dan tandai status cetak struk.
- Hak minimal: tulis `timbangan` + `fee.status_cetak`; tidak boleh baca data pengguna.
- Rotasi: ganti per gudang per 90 hari; kunci lama diberi masa tenggang 7 hari.

## Service API Key

- Header: `X-Service-Key: <SERVICE_API_KEY>`.
- Dipakai `lumbung-data-pipeline` dan serving `lumbung-ai-models` untuk ingest dan baca agregat.
- Hak minimal: tulis pipeline + baca agregat; tanpa akses auth pengguna.
- Simpan sebagai GitHub Actions secret; jangan commit ke repo.
