//! Enum domain bersama (FSD/20-model-data): komoditas pilot dan tarif tetap.

use serde::{Deserialize, Serialize};

/// 3 komoditas pilot v1.0.0 (KONTEKS §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Komoditas {
    /// Cabai.
    Cabai,
    /// Bawang.
    Bawang,
    /// Kangkung.
    Kangkung,
}

impl Komoditas {
    /// Semua komoditas pilot.
    pub const SEMUA: [Komoditas; 3] = [Komoditas::Cabai, Komoditas::Bawang, Komoditas::Kangkung];

    /// Nilai teks seperti di basis data dan kontrak.
    pub fn sebagai_str(self) -> &'static str {
        match self {
            Komoditas::Cabai => "cabai",
            Komoditas::Bawang => "bawang",
            Komoditas::Kangkung => "kangkung",
        }
    }

    /// Urai dari teks basis data.
    pub fn dari_str(nilai: &str) -> Option<Self> {
        Self::SEMUA.into_iter().find(|k| k.sebagai_str() == nilai)
    }
}

/// Tarif dan persentase terkunci v1.0.0 (BRD/30-keuangan-fee). Angka bisnis hanya dari dokumen.
pub mod tarif {
    /// Fee logistik zona DEKAT (<15 km), rupiah per kg (BR-301).
    pub const FEE_DEKAT_PER_KG: i64 = 1_500;
    /// Fee logistik zona JAUH (15-25 km), rupiah per kg (BR-301).
    pub const FEE_JAUH_PER_KG: i64 = 2_500;
    /// Buffer papan harga: median referensi dikurangi 10 persen (BR-305).
    pub const BUFFER_PERSEN: i64 = 10;
    /// Pembulatan ke bawah papan harga: kelipatan Rp500 (BR-305).
    pub const KELIPATAN_PAPAN: i64 = 500;
    /// Grade B dibayar 85 persen harga Grade A (tiket LMB-BE-006: Grade A 100%, B 85%).
    pub const PERSEN_GRADE_B: i64 = 85;
}
