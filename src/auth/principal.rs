//! Ekstraktor identitas untuk handler Axum.
//!
//! | Ekstraktor | Kredensial | Dipakai untuk |
//! |---|---|---|
//! | [`Pengguna`] | `Authorization: Bearer` (JWT akses atau token shift) | endpoint peran |
//! | [`PerangkatGudang`] | `X-Device-Key` | `POST /gudang/masuk` |
//! | [`Gudang`] | bearer shift/akses **dan** `X-Device-Key` | endpoint desktop gudang |
//! | [`Layanan`] | `X-Service-Key` | server-ke-server (web, pipeline, AI) |
//! | [`AkunAdmin`] | bearer peran admin **atau** `X-Service-Key` | endpoint `/admin/*` |
//!
//! Least privilege: kunci perangkat dan kunci layanan tidak pernah menggantikan bearer
//! pada endpoint data pengguna; peran yang salah mendapat 403 dan dicatat di audit.

use axum::{extract::FromRequestParts, http::request::Parts};
use serde_json::json;
use uuid::Uuid;

use crate::{
    audit,
    auth::{kunci, peran::Peran, sama_waktu_konstan},
    domain::Konteks,
    error::AppError,
    state::AppState,
};

/// Pengguna terautentikasi lewat JWT.
#[derive(Debug, Clone)]
pub struct Pengguna {
    /// ID pengguna.
    pub id: Uuid,
    /// Akun peran, contoh `sopir-02`.
    pub akun: String,
    /// Peran.
    pub peran: Peran,
    /// Keluarga refresh (token akses biasa).
    pub keluarga: Option<Uuid>,
    /// `jti` sesi shift (token shift gudang).
    pub shift_jti: Option<Uuid>,
    /// Nilai header `X-Perangkat-Id`.
    pub perangkat_id: Option<String>,
}

impl Pengguna {
    /// Konteks audit untuk fungsi layanan.
    pub fn konteks(&self) -> Konteks {
        Konteks {
            akun: self.akun.clone(),
            perangkat: self.perangkat_id.clone(),
        }
    }

    /// Wajibkan salah satu peran; bila tidak, catat penolakan di audit lalu 403.
    pub async fn wajib_peran(&self, state: &AppState, boleh: &[Peran]) -> Result<(), AppError> {
        if boleh.contains(&self.peran) {
            return Ok(());
        }
        let mut conn = state.pool.acquire().await?;
        audit::catat(
            &mut conn,
            &self.akun,
            "akses_ditolak",
            self.perangkat_id.as_deref(),
            json!({
                "peran": self.peran.sebagai_str(),
                "diizinkan": boleh.iter().map(|p| p.sebagai_str()).collect::<Vec<_>>(),
            }),
        )
        .await?;
        Err(AppError::Forbidden)
    }
}

fn header<'a>(parts: &'a Parts, nama: &str) -> Option<&'a str> {
    parts.headers.get(nama).and_then(|v| v.to_str().ok())
}

fn perangkat(parts: &Parts) -> Option<String> {
    header(parts, "x-perangkat-id")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn bearer(parts: &Parts) -> Option<&str> {
    let nilai = header(parts, "authorization")?;
    let (skema, token) = nilai.split_once(' ')?;
    if skema.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() {
        Some(token.trim())
    } else {
        None
    }
}

impl FromRequestParts<AppState> for Pengguna {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer(parts).ok_or(AppError::Unauthorized)?;
        let klaim = state.jwt.verifikasi(token)?;
        let id = Uuid::parse_str(&klaim.sub).map_err(|_| AppError::Unauthorized)?;
        let peran = Peran::dari_str(&klaim.peran).ok_or(AppError::Unauthorized)?;

        // Akun yang dinonaktifkan langsung kehilangan akses (tanpa menunggu token habis).
        let aktif: Option<bool> = sqlx::query_scalar("SELECT aktif FROM pengguna WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
        if aktif != Some(true) {
            return Err(AppError::Unauthorized);
        }

        let jti = Uuid::parse_str(&klaim.jti).map_err(|_| AppError::Unauthorized)?;
        let shift_jti = if klaim.shift {
            let sah: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM sesi_shift
                 WHERE jti = $1 AND revoked_at IS NULL AND expires_at > now())",
            )
            .bind(jti)
            .fetch_one(&state.pool)
            .await?;
            if !sah {
                return Err(AppError::Unauthorized);
            }
            Some(jti)
        } else {
            None
        };
        let keluarga = klaim
            .fam
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()
            .map_err(|_| AppError::Unauthorized)?;

        Ok(Pengguna {
            id,
            akun: klaim.akun,
            peran,
            keluarga,
            shift_jti,
            perangkat_id: perangkat(parts),
        })
    }
}

/// Perangkat gudang terdaftar lewat `X-Device-Key`.
#[derive(Debug, Clone)]
pub struct PerangkatGudang(pub kunci::KunciPerangkat);

impl FromRequestParts<AppState> for PerangkatGudang {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let nilai = header(parts, "x-device-key").ok_or(AppError::Unauthorized)?;
        let k = kunci::verifikasi(&state.pool, nilai.trim())
            .await?
            .ok_or(AppError::Unauthorized)?;
        Ok(PerangkatGudang(k))
    }
}

/// Desktop gudang: bearer (token shift atau akses) **dan** `X-Device-Key` sah.
#[derive(Debug, Clone)]
pub struct Gudang {
    /// Operator/admin yang sedang bertugas.
    pub pengguna: Pengguna,
    /// Kunci perangkat PC gudang.
    pub perangkat: kunci::KunciPerangkat,
}

impl Gudang {
    /// Konteks audit untuk fungsi layanan.
    pub fn konteks(&self) -> Konteks {
        self.pengguna.konteks()
    }
}

impl FromRequestParts<AppState> for Gudang {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let pengguna = Pengguna::from_request_parts(parts, state).await?;
        let PerangkatGudang(perangkat) = PerangkatGudang::from_request_parts(parts, state).await?;
        Ok(Gudang {
            pengguna,
            perangkat,
        })
    }
}

/// Kunci layanan `X-Service-Key` (pipeline, AI serving, web server-side).
#[derive(Debug, Clone, Copy)]
pub struct Layanan;

impl FromRequestParts<AppState> for Layanan {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let nilai = header(parts, "x-service-key").ok_or(AppError::Unauthorized)?;
        if !state.config.service_api_key.is_empty()
            && sama_waktu_konstan(nilai.trim(), &state.config.service_api_key)
        {
            Ok(Layanan)
        } else {
            Err(AppError::Unauthorized)
        }
    }
}

/// Akses endpoint `/admin/*`: sesi pengguna peran admin atau kunci layanan.
#[derive(Debug, Clone)]
pub enum AkunAdmin {
    /// Pengguna admin (admin gudang, admin keuangan, koordinator tani).
    Pengguna(Pengguna),
    /// Server-ke-server memakai `X-Service-Key`.
    Layanan {
        /// Nilai `X-Perangkat-Id` bila dikirim.
        perangkat_id: Option<String>,
    },
}

impl AkunAdmin {
    /// Nama aktor untuk audit.
    pub fn aktor(&self) -> String {
        match self {
            AkunAdmin::Pengguna(p) => p.akun.clone(),
            AkunAdmin::Layanan { .. } => "layanan-web".to_string(),
        }
    }

    /// Nilai `X-Perangkat-Id` untuk audit.
    pub fn perangkat_id(&self) -> Option<&str> {
        match self {
            AkunAdmin::Pengguna(p) => p.perangkat_id.as_deref(),
            AkunAdmin::Layanan { perangkat_id } => perangkat_id.as_deref(),
        }
    }

    /// Kunci layanan selalu lolos; pengguna harus berperan salah satu dari `boleh`.
    pub async fn wajib_peran(&self, state: &AppState, boleh: &[Peran]) -> Result<(), AppError> {
        match self {
            AkunAdmin::Layanan { .. } => Ok(()),
            AkunAdmin::Pengguna(p) => p.wajib_peran(state, boleh).await,
        }
    }
}

impl FromRequestParts<AppState> for AkunAdmin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if header(parts, "x-service-key").is_some() {
            Layanan::from_request_parts(parts, state).await?;
            return Ok(AkunAdmin::Layanan {
                perangkat_id: perangkat(parts),
            });
        }
        let pengguna = Pengguna::from_request_parts(parts, state).await?;
        // Semua peran admin lolos ekstraksi; endpoint mempersempit lewat `wajib_peran`.
        pengguna
            .wajib_peran(
                state,
                &[
                    Peran::AdminGudang,
                    Peran::AdminKeuangan,
                    Peran::KoordinatorTani,
                ],
            )
            .await?;
        Ok(AkunAdmin::Pengguna(pengguna))
    }
}

/// Operator gudang: bearer + `X-Device-Key` sah dan peran `admin_gudang`. Peran diperiksa sebelum
/// badan permintaan dibaca, sehingga peran salah selalu 403 (bukan 422 dari validasi badan).
#[derive(Debug, Clone)]
pub struct OperatorGudang(pub Gudang);

impl FromRequestParts<AppState> for OperatorGudang {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let gudang = Gudang::from_request_parts(parts, state).await?;
        gudang
            .pengguna
            .wajib_peran(state, &[Peran::AdminGudang])
            .await?;
        Ok(OperatorGudang(gudang))
    }
}

/// Sopir terautentikasi (bearer, peran `sopir`); peran diperiksa sebelum badan dibaca.
#[derive(Debug, Clone)]
pub struct PeranSopir(pub Pengguna);

impl FromRequestParts<AppState> for PeranSopir {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let pengguna = Pengguna::from_request_parts(parts, state).await?;
        pengguna.wajib_peran(state, &[Peran::Sopir]).await?;
        Ok(PeranSopir(pengguna))
    }
}

/// Admin gudang lewat sesi bearer atau kunci layanan (web); peran diperiksa sebelum badan dibaca.
#[derive(Debug, Clone)]
pub struct AdminGudangAkses(pub AkunAdmin);

impl FromRequestParts<AppState> for AdminGudangAkses {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let akun = AkunAdmin::from_request_parts(parts, state).await?;
        akun.wajib_peran(state, &[Peran::AdminGudang]).await?;
        Ok(AdminGudangAkses(akun))
    }
}

/// Petani terautentikasi (bearer, peran `petani`); peran diperiksa sebelum badan dibaca.
#[derive(Debug, Clone)]
pub struct PeranPetani(pub Pengguna);

impl FromRequestParts<AppState> for PeranPetani {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let pengguna = Pengguna::from_request_parts(parts, state).await?;
        pengguna.wajib_peran(state, &[Peran::Petani]).await?;
        Ok(PeranPetani(pengguna))
    }
}

/// Admin keuangan atau admin gudang lewat sesi bearer, atau kunci layanan (web); peran diperiksa
/// sebelum badan dibaca. Koordinator tani tidak punya akses invoice.
#[derive(Debug, Clone)]
pub struct AdminKeuanganAkses(pub AkunAdmin);

impl FromRequestParts<AppState> for AdminKeuanganAkses {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let akun = AkunAdmin::from_request_parts(parts, state).await?;
        akun.wajib_peran(state, &[Peran::AdminKeuangan, Peran::AdminGudang])
            .await?;
        Ok(AdminKeuanganAkses(akun))
    }
}
