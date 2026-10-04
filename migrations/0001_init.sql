-- Migrasi awal basis data `lumbung` (Postgres khusus Lumbung).
-- Tabel minimal: panen, lot, trip, timbangan, fee.

CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- Panen: catatan panen per kebun per tanggal.
CREATE TABLE panen (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kebun TEXT NOT NULL,
    komoditas TEXT NOT NULL,
    tanggal DATE NOT NULL,
    berat_kebun_kg NUMERIC(10, 2) NOT NULL CHECK (berat_kebun_kg >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Lot: unit dagang yang dibawa dari kebun ke gudang.
CREATE TABLE lot (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    panen_id UUID REFERENCES panen(id) ON DELETE SET NULL,
    kode TEXT NOT NULL UNIQUE,
    komoditas TEXT NOT NULL,
    asal_kebun TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'dibuat' CHECK (status IN ('dibuat', 'diantar', 'diterima', 'ditimbang', 'selesai')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Trip: perjalanan kebun ke gudang / gudang ke pasar.
CREATE TABLE trip (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    asal TEXT NOT NULL,
    tujuan TEXT NOT NULL,
    kendaraan TEXT NOT NULL DEFAULT '',
    berangkat_at TIMESTAMPTZ,
    tiba_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Timbangan: hasil timbang gudang per lot (sumber USB/serial).
CREATE TABLE timbangan (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lot_id UUID NOT NULL REFERENCES lot(id) ON DELETE CASCADE,
    berat_gudang_kg NUMERIC(10, 2) NOT NULL CHECK (berat_gudang_kg >= 0),
    susut_kg NUMERIC(10, 2) NOT NULL DEFAULT 0,
    ditimbang_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Fee: biaya layanan gudang per lot.
CREATE TABLE fee (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lot_id UUID NOT NULL REFERENCES lot(id) ON DELETE CASCADE,
    timbangan_id UUID REFERENCES timbangan(id) ON DELETE SET NULL,
    jumlah NUMERIC(12, 2) NOT NULL CHECK (jumlah >= 0),
    status_cetak TEXT NOT NULL DEFAULT 'belum' CHECK (status_cetak IN ('belum', 'sudah')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_lot_status ON lot(status);
CREATE INDEX idx_timbangan_lot ON timbangan(lot_id);
CREATE INDEX idx_fee_lot ON fee(lot_id);
