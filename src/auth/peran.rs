//! Peran pengguna (PRD/10-pengguna): satu orang satu akun, satu peran.

use serde::{Deserialize, Serialize};

/// Peran yang dikenal kontrak (`TokenResponse.peran`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Peran {
    /// Petani mitra: hanya data miliknya sendiri.
    Petani,
    /// Sopir: hanya trip hari itu.
    Sopir,
    /// Admin gudang (termasuk operator sortir/timbang): lot, trip, papan harga.
    AdminGudang,
    /// Admin keuangan: invoice dan rekap.
    AdminKeuangan,
    /// Koordinator tani: mitra dan kontrak.
    KoordinatorTani,
}

impl Peran {
    /// Nilai teks seperti di basis data dan kontrak.
    pub fn sebagai_str(self) -> &'static str {
        match self {
            Peran::Petani => "petani",
            Peran::Sopir => "sopir",
            Peran::AdminGudang => "admin_gudang",
            Peran::AdminKeuangan => "admin_keuangan",
            Peran::KoordinatorTani => "koordinator_tani",
        }
    }

    /// Urai dari teks; `None` bila tidak dikenal.
    pub fn dari_str(nilai: &str) -> Option<Self> {
        match nilai {
            "petani" => Some(Peran::Petani),
            "sopir" => Some(Peran::Sopir),
            "admin_gudang" => Some(Peran::AdminGudang),
            "admin_keuangan" => Some(Peran::AdminKeuangan),
            "koordinator_tani" => Some(Peran::KoordinatorTani),
            _ => None,
        }
    }

    /// Peran yang boleh masuk web admin.
    pub fn adalah_admin(self) -> bool {
        matches!(
            self,
            Peran::AdminGudang | Peran::AdminKeuangan | Peran::KoordinatorTani
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Peran;

    #[test]
    fn bolak_balik_teks() {
        for p in [
            Peran::Petani,
            Peran::Sopir,
            Peran::AdminGudang,
            Peran::AdminKeuangan,
            Peran::KoordinatorTani,
        ] {
            assert_eq!(Peran::dari_str(p.sebagai_str()), Some(p));
        }
        assert_eq!(Peran::dari_str("root"), None);
    }
}
