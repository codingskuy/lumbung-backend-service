-- LMB-BE-008: tarif fee per kg dapat diubah admin (BR-306: maks 1x per semester, maks 10 persen,
-- pemberitahuan 30 hari). Baris awal memuat tarif BR-301 (Rp1.500 dekat, Rp2.500 jauh).
CREATE TABLE tarif_fee (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uuid_klien UUID UNIQUE,
    berlaku_mulai DATE NOT NULL UNIQUE,
    dekat BIGINT NOT NULL CHECK (dekat > 0),
    jauh BIGINT NOT NULL CHECK (jauh > 0),
    diterbitkan_oleh TEXT NOT NULL,
    audit_id TEXT,
    dibuat_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO tarif_fee (berlaku_mulai, dekat, jauh, diterbitkan_oleh)
VALUES ('2000-01-01', 1500, 2500, 'awal-BR-301');

-- Fee dan papan memuat tarif yang berlaku saat itu; tarif tidak lagi dibatasi 1500/2500.
ALTER TABLE fee DROP CONSTRAINT fee_tarif_per_kg_check;
ALTER TABLE fee ADD CONSTRAINT fee_tarif_per_kg_check CHECK (tarif_per_kg > 0);
