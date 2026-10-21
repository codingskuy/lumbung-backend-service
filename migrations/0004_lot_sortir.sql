-- LMB-BE-004: lot, sortir A/B/C (+-2%), keranjang batch, stok-lot. Menyelaraskan skema 0001 (templat
-- berat kg desimal dan ID uuid) dengan kontrak v1.0.0 (gram integer, kode lot L-NNNN) sesuai ADR-0001.
-- Tabel templat 0001 belum dipakai kode mana pun dan tidak berisi data; dibuang lalu dibuat ulang.

DROP TABLE IF EXISTS fee;
DROP TABLE IF EXISTS timbangan;
DROP TABLE IF EXISTS lot;
DROP TABLE IF EXISTS panen;

-- Petani mitra (kode PTN-NNN = akun peran petani di tabel pengguna).
CREATE TABLE petani (
    kode TEXT PRIMARY KEY CHECK (kode ~ '^PTN-[0-9]{3}$'),
    nama TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'AKTIF' CHECK (status IN ('AKTIF', 'NONAKTIF')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Nomor resmi lot L-NNNN dari server (global, tidak mengulang).
CREATE SEQUENCE lot_kode_seq;

CREATE TABLE lot (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uuid_klien UUID NOT NULL UNIQUE,
    kode TEXT NOT NULL UNIQUE,
    -- Nomor sementara TMP-{uuid8} dari klien offline yang dipetakan ke `kode` (LMB-BE-005).
    nomor_sementara TEXT UNIQUE CHECK (nomor_sementara ~ '^TMP-[0-9a-f]{8}$'),
    kode_petani TEXT NOT NULL REFERENCES petani(kode),
    komoditas TEXT NOT NULL CHECK (komoditas IN ('cabai', 'bawang', 'kangkung')),
    tanggal DATE NOT NULL,
    berat_kotor_g BIGINT NOT NULL CHECK (berat_kotor_g > 0),
    grade_a_g BIGINT CHECK (grade_a_g >= 0),
    grade_b_g BIGINT CHECK (grade_b_g >= 0),
    grade_c_g BIGINT CHECK (grade_c_g >= 0),
    status TEXT NOT NULL DEFAULT 'BARU' CHECK (status IN ('BARU', 'DISORTIR', 'DIMUAT', 'DITERIMA')),
    versi INTEGER NOT NULL DEFAULT 1 CHECK (versi >= 1),
    dibuat_oleh TEXT NOT NULL,
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    diperbarui_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Grade terisi seluruhnya atau kosong seluruhnya, dan hanya lot BARU yang belum disortir.
    CHECK ((grade_a_g IS NULL) = (grade_b_g IS NULL) AND (grade_a_g IS NULL) = (grade_c_g IS NULL)),
    CHECK ((status = 'BARU') = (grade_a_g IS NULL))
);
CREATE INDEX idx_lot_status ON lot(status);
CREATE INDEX idx_lot_petani ON lot(kode_petani);

-- Timbangan (FSD/20): KOTOR saat lot dibuat, BERSIH = total hasil sortir (A+B+C).
CREATE TABLE timbangan (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lot_id UUID NOT NULL REFERENCES lot(id) ON DELETE CASCADE,
    jenis TEXT NOT NULL CHECK (jenis IN ('KOTOR', 'BERSIH')),
    berat_g BIGINT NOT NULL CHECK (berat_g >= 0),
    petugas TEXT NOT NULL,
    waktu TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (lot_id, jenis)
);

-- Sortir: satu per lot; uuid_klien menjaga idempotensi.
CREATE TABLE sortir (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uuid_klien UUID NOT NULL UNIQUE,
    lot_id UUID NOT NULL UNIQUE REFERENCES lot(id) ON DELETE CASCADE,
    grade_a_g BIGINT NOT NULL CHECK (grade_a_g >= 0),
    grade_b_g BIGINT NOT NULL CHECK (grade_b_g >= 0),
    grade_c_g BIGINT NOT NULL CHECK (grade_c_g >= 0),
    petugas TEXT NOT NULL,
    waktu TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Alih Grade C: berat yang tidak masuk rantai dingin (FR-003) dan alasannya.
CREATE TABLE alih_grade_c (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lot_id UUID NOT NULL REFERENCES lot(id) ON DELETE CASCADE,
    berat_g BIGINT NOT NULL CHECK (berat_g > 0),
    alasan TEXT NOT NULL DEFAULT 'Grade C tidak masuk rantai dingin',
    dicatat_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_alih_lot ON alih_grade_c(lot_id);

-- Batch keranjang (idempotensi per UUID klien) dan isinya. Kode KRJ-001..KRJ-120 menandai keranjang
-- fisik yang dipakai berulang: satu kode hanya boleh punya satu isi aktif (selesai_at kosong).
CREATE TABLE keranjang_batch (
    uuid_klien UUID PRIMARY KEY,
    jumlah INTEGER NOT NULL CHECK (jumlah BETWEEN 1 AND 50),
    dibuat_oleh TEXT NOT NULL,
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE keranjang_isi (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    batch_uuid UUID NOT NULL REFERENCES keranjang_batch(uuid_klien) ON DELETE CASCADE,
    kode TEXT NOT NULL CHECK (kode ~ '^KRJ-[0-9]{3}$'),
    lot_id UUID NOT NULL REFERENCES lot(id),
    grade TEXT NOT NULL CHECK (grade IN ('A', 'B')),
    berat_g BIGINT NOT NULL CHECK (berat_g > 0),
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    selesai_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX uq_keranjang_aktif ON keranjang_isi(kode) WHERE selesai_at IS NULL;
CREATE INDEX idx_keranjang_lot ON keranjang_isi(lot_id, grade);
