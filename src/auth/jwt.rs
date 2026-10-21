//! JWT akses: ed25519 (EdDSA), audiens `lumbung`, umur 15 menit.
//! Token shift gudang memakai klaim yang sama dengan `shift = true` dan umur 12 jam.

use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

/// Audiens JWT Lumbung (SRS/30: audiens dikunci).
pub const AUDIENS: &str = "lumbung";
/// Penerbit JWT.
pub const PENERBIT: &str = "lumbung-backend-service";
/// Umur token akses: 15 menit.
pub const UMUR_AKSES_MENIT: i64 = 15;
/// Umur token shift gudang: 12 jam.
pub const UMUR_SHIFT_JAM: i64 = 12;
/// Umur refresh token: 7 hari.
pub const UMUR_REFRESH_HARI: i64 = 7;
/// Toleransi selisih jam (detik) saat memeriksa `exp`.
const TOLERANSI_DETIK: u64 = 5;

/// Klaim JWT Lumbung.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Klaim {
    /// ID pengguna (UUID).
    pub sub: String,
    /// Audiens, selalu `lumbung`.
    pub aud: String,
    /// Penerbit.
    pub iss: String,
    /// Waktu terbit (detik epoch).
    pub iat: i64,
    /// Waktu kedaluwarsa (detik epoch).
    pub exp: i64,
    /// ID token unik.
    pub jti: String,
    /// Akun peran (contoh `sopir-02`).
    pub akun: String,
    /// Peran (snake_case).
    pub peran: String,
    /// Keluarga refresh (hanya token akses biasa).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fam: Option<String>,
    /// Token shift gudang.
    #[serde(default)]
    pub shift: bool,
}

/// Pasangan kunci penanda dan verifikasi.
pub struct JwtKeys {
    enkode: EncodingKey,
    dekode: DecodingKey,
}

impl std::fmt::Debug for JwtKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtKeys").finish_non_exhaustive()
    }
}

impl JwtKeys {
    /// Bangun dari PEM privat (PKCS#8) dan publik (SPKI) ed25519.
    pub fn dari_pem(privat: &str, publik: &str) -> anyhow::Result<Self> {
        let enkode = EncodingKey::from_ed_pem(privat.as_bytes())
            .map_err(|e| anyhow::anyhow!("JWT_ED25519_PRIVATE bukan PEM ed25519 valid: {e}"))?;
        let dekode = DecodingKey::from_ed_pem(publik.as_bytes())
            .map_err(|e| anyhow::anyhow!("JWT_ED25519_PUBLIC bukan PEM ed25519 valid: {e}"))?;
        Ok(Self { enkode, dekode })
    }

    /// Terbitkan token dengan umur tertentu (negatif dipakai uji untuk token kedaluwarsa).
    pub fn terbitkan(
        &self,
        pengguna: Uuid,
        akun: &str,
        peran: &str,
        fam: Option<Uuid>,
        shift: bool,
        umur: Duration,
    ) -> Result<(String, Klaim), AppError> {
        let sekarang = Utc::now();
        let klaim = Klaim {
            sub: pengguna.to_string(),
            aud: AUDIENS.to_string(),
            iss: PENERBIT.to_string(),
            iat: sekarang.timestamp(),
            exp: (sekarang + umur).timestamp(),
            jti: Uuid::now_v7().to_string(),
            akun: akun.to_string(),
            peran: peran.to_string(),
            fam: fam.map(|f| f.to_string()),
            shift,
        };
        let token = jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), &klaim, &self.enkode)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("gagal menandatangani JWT: {e}")))?;
        Ok((token, klaim))
    }

    /// Terbitkan token akses 15 menit.
    pub fn terbitkan_akses(
        &self,
        pengguna: Uuid,
        akun: &str,
        peran: &str,
        fam: Uuid,
    ) -> Result<(String, Klaim), AppError> {
        self.terbitkan(
            pengguna,
            akun,
            peran,
            Some(fam),
            false,
            Duration::minutes(UMUR_AKSES_MENIT),
        )
    }

    /// Terbitkan token shift gudang 12 jam.
    pub fn terbitkan_shift(
        &self,
        pengguna: Uuid,
        akun: &str,
        peran: &str,
    ) -> Result<(String, Klaim), AppError> {
        self.terbitkan(
            pengguna,
            akun,
            peran,
            None,
            true,
            Duration::hours(UMUR_SHIFT_JAM),
        )
    }

    /// Verifikasi tanda tangan, kedaluwarsa, dan audiens; apa pun gagalnya menjadi 401.
    pub fn verifikasi(&self, token: &str) -> Result<Klaim, AppError> {
        let mut v = Validation::new(Algorithm::EdDSA);
        v.set_audience(&[AUDIENS]);
        v.set_issuer(&[PENERBIT]);
        v.set_required_spec_claims(&["exp", "aud", "sub", "iss"]);
        v.leeway = TOLERANSI_DETIK;
        jsonwebtoken::decode::<Klaim>(token, &self.dekode, &v)
            .map(|d| d.claims)
            .map_err(|_| AppError::Unauthorized)
    }
}

/// Waktu kedaluwarsa klaim sebagai `DateTime<Utc>`.
pub fn exp_sebagai_waktu(klaim: &Klaim) -> DateTime<Utc> {
    DateTime::from_timestamp(klaim.exp, 0).unwrap_or_else(Utc::now)
}
