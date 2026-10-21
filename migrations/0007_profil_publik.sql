-- LMB-BE-TEST-PACT: profil publik dibaca dari basis data (satu baris), bukan konstanta kode, agar
-- deskripsi dan kontak kemitraan dapat diisi operator tanpa rilis ulang. Tidak ada endpoint tulis
-- (hak tulis admin minimum, ADR-0006): diisi lewat SQL/alat operator.
CREATE TABLE profil_publik (
    id SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    nama TEXT NOT NULL CHECK (length(nama) BETWEEN 1 AND 128),
    deskripsi TEXT CHECK (length(deskripsi) <= 2000),
    kontak_kemitraan TEXT CHECK (length(kontak_kemitraan) <= 256)
);
INSERT INTO profil_publik (id, nama) VALUES (1, 'Lumbung');
