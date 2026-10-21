#!/usr/bin/env python3
"""Seed data uji untuk Schemathesis/Pact lokal (BUKAN untuk produksi).

Membuat petani PTN-007, akun peran (hash argon2id, PIN 123456) lewat psql. Dijalankan lewat:
  uvx --with argon2-cffi python contract/seed.py "$DATABASE_URL"
"""
import subprocess
import sys

from argon2 import PasswordHasher

PIN = "123456"
AKUN = [
    ("OPR-SORTIR-1", "admin_gudang"),
    ("sopir-02", "sopir"),
    ("PTN-007", "petani"),
    ("keu-01", "admin_keuangan"),
]


def psql(url: str, sql: str) -> None:
    subprocess.run(["psql", url, "-v", "ON_ERROR_STOP=1", "-qc", sql], check=True)


def main() -> None:
    url = sys.argv[1]
    h = PasswordHasher().hash(PIN)
    psql(url, "INSERT INTO petani (kode, nama) VALUES ('PTN-007', 'Slamet') ON CONFLICT DO NOTHING")
    for akun, peran in AKUN:
        psql(
            url,
            f"INSERT INTO pengguna (akun, nama, peran, pin_hash) VALUES ('{akun}', '{akun}', '{peran}', '{h}') "
            "ON CONFLICT (akun) DO UPDATE SET pin_hash = EXCLUDED.pin_hash, gagal_pin = 0, terkunci_sampai = NULL, aktif = true",
        )
    print("seed selesai:", ", ".join(a for a, _ in AKUN))


if __name__ == "__main__":
    main()
