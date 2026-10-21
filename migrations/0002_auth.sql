-- LMB-BE-003: autentikasi pengguna (JWT + refresh), shift gudang, kunci perangkat, audit, urutan nomor harian.
-- Fungsi digest() berasal dari ekstensi pgcrypto (sudah dibuat di 0001_init.sql).

-- Pengguna: satu orang satu akun (akun peran + PIN 6 digit, disimpan sebagai hash argon2).
CREATE TABLE pengguna (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    akun TEXT NOT NULL UNIQUE,
    nama TEXT NOT NULL DEFAULT '',
    peran TEXT NOT NULL CHECK (peran IN ('petani', 'sopir', 'admin_gudang', 'admin_keuangan', 'koordinator_tani')),
    pin_hash TEXT NOT NULL,
    pin_diganti_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    gagal_pin INTEGER NOT NULL DEFAULT 0 CHECK (gagal_pin >= 0),
    terkunci_sampai TIMESTAMPTZ,
    aktif BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Refresh token: hanya hash SHA-256 yang disimpan; family_id mengikat satu rantai rotasi.
CREATE TABLE refresh_token (
    id UUID PRIMARY KEY,
    family_id UUID NOT NULL,
    pengguna_id UUID NOT NULL REFERENCES pengguna(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    perangkat_id TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_refresh_family ON refresh_token(family_id);
CREATE INDEX idx_refresh_pengguna ON refresh_token(pengguna_id);

-- Kunci API perangkat gudang: hanya hash; berlaku 90 hari, kunci lama diberi tenggang 7 hari saat rotasi.
CREATE TABLE kunci_perangkat (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    label TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    berlaku_sampai TIMESTAMPTZ NOT NULL,
    dicabut_at TIMESTAMPTZ
);
CREATE INDEX idx_kunci_label ON kunci_perangkat(label);

-- Sesi shift gudang (token 12 jam) agar bisa dicabut saat tutup shift.
CREATE TABLE sesi_shift (
    jti UUID PRIMARY KEY,
    pengguna_id UUID NOT NULL REFERENCES pengguna(id) ON DELETE CASCADE,
    kunci_id UUID REFERENCES kunci_perangkat(id) ON DELETE SET NULL,
    perangkat_id TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Log audit: login, penolakan 403, dan penulisan admin (audit_id AUD-YYYYMMDD-NNN).
CREATE TABLE audit_log (
    id BIGSERIAL PRIMARY KEY,
    audit_id TEXT UNIQUE,
    waktu TIMESTAMPTZ NOT NULL DEFAULT now(),
    aktor TEXT NOT NULL,
    aksi TEXT NOT NULL,
    perangkat TEXT,
    detail JSONB NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX idx_audit_waktu ON audit_log(waktu);

-- Penghitung urutan harian untuk nomor server (AUD-, SJ-, INS-): atomik via upsert.
CREATE TABLE urutan_harian (
    jenis TEXT NOT NULL,
    tanggal DATE NOT NULL,
    nilai INTEGER NOT NULL,
    PRIMARY KEY (jenis, tanggal)
);
