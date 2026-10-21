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

/// Grade mutu hasil sortir (FSD/20): A dan B masuk rantai dingin, C dialihkan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Grade {
    /// Grade A (dibayar 100 persen harga papan).
    A,
    /// Grade B (dibayar 85 persen harga papan).
    B,
    /// Grade C (tidak masuk rantai dingin; alihnya dicatat, FR-003).
    C,
}

impl Grade {
    /// Nilai teks seperti di basis data dan kontrak.
    pub fn sebagai_str(self) -> &'static str {
        match self {
            Grade::A => "A",
            Grade::B => "B",
            Grade::C => "C",
        }
    }
}

/// Zona fee logistik (BR-301).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Zona {
    /// Kurang dari 15 km: Rp1.500 per kg.
    Dekat,
    /// 15 sampai 25 km: Rp2.500 per kg.
    Jauh,
}

impl Zona {
    /// Teks basis data/kontrak.
    pub fn sebagai_str(self) -> &'static str {
        match self {
            Zona::Dekat => "DEKAT",
            Zona::Jauh => "JAUH",
        }
    }

    /// Urai dari teks basis data.
    pub fn dari_str(nilai: &str) -> Option<Self> {
        match nilai {
            "DEKAT" => Some(Zona::Dekat),
            "JAUH" => Some(Zona::Jauh),
            _ => None,
        }
    }

    /// Tarif fee per kg zona ini (rupiah).
    pub fn tarif_per_kg(self) -> i64 {
        match self {
            Zona::Dekat => tarif::FEE_DEKAT_PER_KG,
            Zona::Jauh => tarif::FEE_JAUH_PER_KG,
        }
    }
}

/// Hasil aksi idempoten (`AksiHasil` pada kontrak): `duplikat` true bila UUID klien sudah diproses.
#[derive(Debug, Serialize)]
pub struct AksiHasil {
    /// ID rekaman hasil.
    pub id: String,
    /// `true` bila UUID klien sama sudah pernah diproses.
    pub duplikat: bool,
}

/// Siapa yang bertindak dan dari perangkat mana (untuk audit dan kolom `dibuat_oleh`).
#[derive(Debug, Clone)]
pub struct Konteks {
    /// Akun pelaku.
    pub akun: String,
    /// Nilai `X-Perangkat-Id` bila ada.
    pub perangkat: Option<String>,
}

/// Hasil operasi idempoten: rekaman baru, atau UUID klien sudah pernah diproses (id hasil pertama).
#[derive(Debug)]
pub enum Hasil<T> {
    /// Rekaman baru dibuat.
    Baru(T),
    /// UUID klien sama sudah diproses; berisi ID rekaman pertama.
    Duplikat(String),
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
    /// Kas jalan per trip, rupiah (FSD/20 Trip).
    pub const KAS_JALAN_PER_TRIP: i64 = 300_000;
    /// Grade B dibayar 85 persen harga Grade A (tiket LMB-BE-006: Grade A 100%, B 85%).
    pub const PERSEN_GRADE_B: i64 = 85;
}
