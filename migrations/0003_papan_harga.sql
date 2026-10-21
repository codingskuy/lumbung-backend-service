-- LMB-BE-007: papan harga mingguan (3 komoditas x grade A/B + fee 2 zona), berlaku Senin-Minggu.

CREATE TABLE papan_harga (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uuid_klien UUID NOT NULL UNIQUE,
    berlaku_mulai DATE NOT NULL UNIQUE CHECK (EXTRACT(ISODOW FROM berlaku_mulai) = 1),
    berlaku_selesai DATE NOT NULL,
    fee_dekat BIGINT NOT NULL CHECK (fee_dekat >= 0),
    fee_jauh BIGINT NOT NULL CHECK (fee_jauh >= 0),
    -- Lembar kerja 3 harga referensi per komoditas (disimpan 6 bulan, BRD/30 Metrik).
    referensi JSONB NOT NULL,
    audit_id TEXT NOT NULL,
    diterbitkan_oleh TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (berlaku_selesai = berlaku_mulai + 6)
);

-- Harga per komoditas dan grade (rupiah per kg, integer). Grade C tidak dipapan.
CREATE TABLE papan_harga_item (
    papan_id UUID NOT NULL REFERENCES papan_harga(id) ON DELETE CASCADE,
    komoditas TEXT NOT NULL CHECK (komoditas IN ('cabai', 'bawang', 'kangkung')),
    grade TEXT NOT NULL CHECK (grade IN ('A', 'B')),
    harga BIGINT NOT NULL CHECK (harga >= 0),
    PRIMARY KEY (papan_id, komoditas, grade)
);
