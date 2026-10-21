# Auth Lumbung

## JWT Pengguna

- Token akses: JWT berumur 15 menit, ditandatangani dengan kunci ed25519 (`JWT_ED25519_PRIVATE`).
- Token refresh: umur 7 hari, disimpan hash di basis data.
- Refresh memakai deteksi pakai-ulang (reuse detection): bila refresh token lama dipakai dua kali, seluruh keluarga refresh milik pengguna dicabut dan pengguna wajib masuk ulang.
- Alur keluar: hapus keluarga refresh dari DB; token akses kedaluwarsa sendiri dalam 15 menit.
- Penandatanganan: ed25519 (EdDSA), audiens `lumbung`, penerbit `lumbung-backend-service`. Layanan membaca
  `JWT_ED25519_PRIVATE` (PEM PKCS#8) untuk menandatangani dan `JWT_ED25519_PUBLIC` (PEM SPKI) untuk verifikasi.
- Refresh token berformat `<id>.<rahasia>` (rahasia 256 bit acak); di DB hanya SHA-256 rahasia. Tiap pemakaian
  merotasi token (token lama ditandai terpakai) dalam keluarga yang sama.
- PIN 6 digit disimpan sebagai hash argon2id. Tiga salah PIN berturut-turut mengunci akun 15 menit (`429`);
  PIN benar mereset hitungan. Akun yang dinonaktifkan kehilangan akses seketika (token akses diperiksa terhadap `pengguna.aktif`).
- Audit (`audit_log`): login, gagal masuk, penguncian, pemakaian ulang refresh, dan penolakan 403 dicatat
  (siapa, perangkat dari `X-Perangkat-Id`, waktu). Penulisan admin mengembalikan `audit_id` `AUD-YYYYMMDD-NNN`.

## Device API Key Gudang

- Header: `X-Device-Key: <DEVICE_API_KEY_GUDANG>`.
- Dipakai perangkat/layanan `gudang-desktop` untuk kirim timbangan dan tandai status cetak struk.
- Hak minimal: tulis `timbangan` + `fee.status_cetak`; tidak boleh baca data pengguna. Kunci perangkat tidak pernah
  menggantikan bearer: endpoint data pengguna menjawab 401 bila hanya `X-Device-Key` yang dikirim.
- Penyimpanan: tabel `kunci_perangkat` hanya berisi hash SHA-256. `DEVICE_API_KEY_GUDANG` didaftarkan saat layanan
  menyala bila belum dikenal (idempoten).
- Rotasi: kunci berlaku 90 hari sejak didaftarkan. Mengganti nilai `DEVICE_API_KEY_GUDANG` lalu me-restart layanan
  mendaftarkan kunci baru (90 hari) dan membatasi kunci lama label yang sama menjadi paling lama 7 hari (masa tenggang).
  Kunci yang tidak dirotasi mati sendiri di hari ke-90. Pencabutan seketika (perangkat hilang) lewat `kunci::cabut`.
- Login shift: `POST /v1/gudang/masuk` (kunci perangkat + PIN operator berperan `admin_gudang`) menerbitkan token shift
  12 jam; `POST /v1/gudang/keluar` mencabutnya (disimpan di `sesi_shift`).

## Service API Key

- Header: `X-Service-Key: <SERVICE_API_KEY>`.
- Dipakai `lumbung-data-pipeline` dan serving `lumbung-ai-models` untuk ingest dan baca agregat.
- Hak minimal: tulis pipeline + baca agregat; tanpa akses auth pengguna. Dibandingkan dalam waktu konstan dan diterima
  hanya pada rute `/admin/*` dan rute layanan; endpoint `/auth/*`, data pengguna, dan endpoint gudang tetap 401.
- Simpan sebagai GitHub Actions secret; jangan commit ke repo.
