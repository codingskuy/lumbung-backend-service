-- LMB-BE-005: trip, surat jalan SJ-YYYYMMDD-NNN, endpoint sopir (tiba/berangkat, bukti terima,
-- foto, insiden) dan dasar sinkron offline. Menyelaraskan tabel `trip` templat 0001 (belum dipakai,
-- tanpa data) dengan kontrak v1.0.0; lihat ADR-0004.

DROP TABLE IF EXISTS trip;

-- Penghitung permanen (tidak direset harian), contoh nomor trip per rute.
CREATE TABLE penghitung (
    jenis TEXT PRIMARY KEY,
    nilai INTEGER NOT NULL
);

CREATE TABLE trip (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uuid_klien UUID NOT NULL UNIQUE,
    kode TEXT NOT NULL UNIQUE CHECK (kode ~ '^TRIP-[A-Z]{2}-[0-9]{3}$'),
    rute TEXT NOT NULL CHECK (rute IN ('UTARA', 'SELATAN', 'KOTA')),
    kendaraan TEXT NOT NULL,
    kas_jalan BIGINT NOT NULL CHECK (kas_jalan >= 0),
    tanggal DATE NOT NULL,
    -- Akun sopir yang bertugas; kosong = belum ditugaskan (tidak tampil di trip-hari-ini).
    sopir_akun TEXT REFERENCES pengguna(akun) ON UPDATE CASCADE ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'DISUSUN' CHECK (status IN ('DISUSUN', 'BERJALAN', 'SELESAI')),
    versi INTEGER NOT NULL DEFAULT 1 CHECK (versi >= 1),
    dibuat_oleh TEXT NOT NULL,
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    diperbarui_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_trip_sopir_tanggal ON trip(sopir_akun, tanggal);

-- Titik tujuan trip berurutan; `pesanan` (PSO-YYYYMMDD-NNN) menjadi dasar fee per pesanan (LMB-BE-006).
CREATE TABLE trip_titik (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    trip_id UUID NOT NULL REFERENCES trip(id) ON DELETE CASCADE,
    titik_id TEXT NOT NULL CHECK (length(titik_id) BETWEEN 1 AND 32),
    urutan INTEGER NOT NULL CHECK (urutan >= 1),
    zona TEXT NOT NULL CHECK (zona IN ('DEKAT', 'JAUH')),
    pesanan TEXT CHECK (pesanan ~ '^PSO-[0-9]{8}-[0-9]{3}$'),
    tiba_at TIMESTAMPTZ,
    berangkat_at TIMESTAMPTZ,
    UNIQUE (trip_id, titik_id),
    UNIQUE (trip_id, urutan)
);

-- Keranjang dimuat ke trip (dan, opsional, ditetapkan ke satu titik tujuan).
ALTER TABLE keranjang_isi
    ADD COLUMN trip_id UUID REFERENCES trip(id),
    ADD COLUMN titik_id TEXT;
CREATE INDEX idx_keranjang_trip ON keranjang_isi(trip_id);

-- Surat jalan: satu per trip; nomor resmi dari server, nomor sementara TMP-* dipetakan.
CREATE TABLE surat_jalan (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uuid_klien UUID NOT NULL UNIQUE,
    trip_id UUID NOT NULL UNIQUE REFERENCES trip(id) ON DELETE CASCADE,
    nomor TEXT NOT NULL UNIQUE CHECK (nomor ~ '^SJ-[0-9]{8}-[0-9]{3}$'),
    nomor_sementara TEXT UNIQUE CHECK (nomor_sementara ~ '^TMP-[0-9a-f]{8}$'),
    diterbitkan_oleh TEXT NOT NULL,
    diterbitkan_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Riwayat tiba/berangkat per titik (idempotensi per UUID klien).
CREATE TABLE titik_event (
    uuid_klien UUID PRIMARY KEY,
    trip_id UUID NOT NULL REFERENCES trip(id) ON DELETE CASCADE,
    titik_id TEXT NOT NULL,
    jenis TEXT NOT NULL CHECK (jenis IN ('TIBA', 'BERANGKAT')),
    waktu TIMESTAMPTZ NOT NULL,
    lat DOUBLE PRECISION CHECK (lat BETWEEN -90 AND 90),
    lng DOUBLE PRECISION CHECK (lng BETWEEN -180 AND 180),
    dicatat_oleh TEXT NOT NULL,
    dicatat_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_titik_event_trip ON titik_event(trip_id, titik_id);

-- Foto bukti: JPEG maksimal 500 KB, disimpan 1 tahun lalu hanya metadata (FSD/20 retensi).
CREATE TABLE foto (
    id UUID PRIMARY KEY,
    pengunggah TEXT NOT NULL,
    ukuran INTEGER NOT NULL CHECK (ukuran BETWEEN 1 AND 512000),
    sha256 TEXT NOT NULL,
    konten BYTEA,
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    simpan_sampai TIMESTAMPTZ NOT NULL DEFAULT now() + interval '1 year'
);

-- Bukti terima per titik. Riwayat disimpan; yang terbaru berlaku (lapangan menang, FSD/10).
CREATE TABLE bukti_terima (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uuid_klien UUID NOT NULL UNIQUE,
    trip_id UUID NOT NULL REFERENCES trip(id) ON DELETE CASCADE,
    titik_id TEXT NOT NULL,
    waktu TIMESTAMPTZ NOT NULL,
    foto_ids UUID[] NOT NULL DEFAULT '{}',
    tanda_tangan TEXT,
    catatan TEXT,
    dicatat_oleh TEXT NOT NULL,
    dicatat_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_bukti_trip_titik ON bukti_terima(trip_id, titik_id, dicatat_at);

-- Insiden INS-YYYYMMDD-NNN; konflik sinkron juga dicatat di sini (jenis `konflik_sinkron`).
CREATE TABLE insiden (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kode TEXT NOT NULL UNIQUE CHECK (kode ~ '^INS-[0-9]{8}-[0-9]{3}$'),
    uuid_klien UUID NOT NULL UNIQUE,
    jenis TEXT NOT NULL CHECK (length(jenis) BETWEEN 1 AND 64),
    catatan TEXT CHECK (length(catatan) <= 2000),
    foto_ids UUID[] NOT NULL DEFAULT '{}',
    dicatat_oleh TEXT NOT NULL,
    dicatat_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_insiden_waktu ON insiden(dicatat_at);
