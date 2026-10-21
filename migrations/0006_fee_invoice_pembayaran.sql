-- LMB-BE-006: fee per kg, invoice mingguan per pembeli, pembayaran petani H+1.
-- Tabel `fee` templat 0001 sudah dibuang di 0004; dibuat ulang sesuai kontrak (gram integer, rupiah integer).

-- Pembeli titik tujuan (opsional); fee hanya ditagihkan bila titik punya pesanan DAN pembeli (ADR-0005).
ALTER TABLE trip_titik
    ADD COLUMN pembeli TEXT CHECK (pembeli IS NULL OR length(pembeli) BETWEEN 1 AND 64);

-- Invoice fee mingguan per pembeli: terbit Senin untuk surat jalan minggu ISO sebelumnya.
CREATE TABLE invoice (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kode TEXT NOT NULL UNIQUE CHECK (kode ~ '^INV-[0-9]{8}-[0-9]{3}$'),
    pembeli TEXT NOT NULL,
    -- Minggu ISO surat jalan yang ditagih, contoh 2026-W41.
    minggu TEXT NOT NULL CHECK (minggu ~ '^[0-9]{4}-W[0-9]{2}$'),
    minggu_mulai DATE NOT NULL CHECK (EXTRACT(ISODOW FROM minggu_mulai) = 1),
    terbit DATE NOT NULL,
    jatuh_tempo DATE NOT NULL,
    nominal BIGINT NOT NULL CHECK (nominal >= 0),
    dilunasi_at TIMESTAMPTZ,
    -- Denda dibekukan saat lunas; sebelum itu dihitung dari tanggal.
    denda BIGINT CHECK (denda >= 0),
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (pembeli, minggu),
    CHECK (jatuh_tempo = terbit + 7)
);
CREATE INDEX idx_invoice_minggu ON invoice(minggu);

-- Fee per pesanan, dihitung saat surat jalan terbit (berat gram x tarif zona per kg).
CREATE TABLE fee (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    surat_jalan_id UUID NOT NULL REFERENCES surat_jalan(id) ON DELETE CASCADE,
    pesanan TEXT NOT NULL CHECK (pesanan ~ '^PSO-[0-9]{8}-[0-9]{3}$'),
    pembeli TEXT,
    berat_g BIGINT NOT NULL CHECK (berat_g > 0),
    zona TEXT NOT NULL CHECK (zona IN ('DEKAT', 'JAUH')),
    tarif_per_kg BIGINT NOT NULL CHECK (tarif_per_kg IN (1500, 2500)),
    nominal BIGINT NOT NULL CHECK (nominal >= 0),
    tanggal DATE NOT NULL,
    status_cetak TEXT NOT NULL DEFAULT 'belum' CHECK (status_cetak IN ('belum', 'sudah')),
    invoice_id UUID REFERENCES invoice(id),
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (surat_jalan_id, pesanan)
);
CREATE INDEX idx_fee_tanggal_belum_diinvoice ON fee(tanggal) WHERE invoice_id IS NULL;
CREATE INDEX idx_fee_invoice ON fee(invoice_id);

-- Pembayaran petani H+1: dibuat saat sortir (berat bersih per grade diketahui); nominal dihitung dari
-- papan harga minggu berjalan pada tanggal sortir, dibekukan saat dibayar.
CREATE TABLE pembayaran_petani (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    lot_id UUID NOT NULL UNIQUE REFERENCES lot(id) ON DELETE CASCADE,
    kode_petani TEXT NOT NULL REFERENCES petani(kode),
    berat_a_g BIGINT NOT NULL CHECK (berat_a_g >= 0),
    berat_b_g BIGINT NOT NULL CHECK (berat_b_g >= 0),
    tanggal DATE NOT NULL,
    jatuh_tempo DATE NOT NULL,
    dibayar_pada TIMESTAMPTZ,
    nominal_dibayar BIGINT CHECK (nominal_dibayar >= 0),
    denda_dibayar BIGINT CHECK (denda_dibayar >= 0),
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (jatuh_tempo = tanggal + 1)
);
CREATE INDEX idx_pembayaran_petani ON pembayaran_petani(kode_petani, tanggal);
