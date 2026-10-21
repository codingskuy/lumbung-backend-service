//! Endpoint autentikasi: `/auth/masuk|refresh|keluar` dan `/gudang/masuk|keluar`.
//!
//! Semua jalur login memakai [`periksa_pin`] sehingga aturan kunci 3 salah PIN = 15 menit
//! berlaku sama untuk akun peran dan operator gudang.

use argon2::password_hash::rand_core::{OsRng, RngCore};
use axum::{extract::State, http::StatusCode, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
    audit,
    auth::{
        jwt::{exp_sebagai_waktu, UMUR_REFRESH_HARI},
        peran::Peran,
        pin,
        principal::{Gudang, Pengguna, PerangkatGudang},
    },
    error::{AppError, AppResult},
    extract::{ApiJson, PerangkatId},
    state::AppState,
};

/// Jumlah salah PIN berturut-turut yang mengunci akun.
pub const BATAS_SALAH_PIN: i32 = 3;
/// Lama kunci akun setelah salah PIN beruntun (menit).
pub const LAMA_KUNCI_MENIT: i64 = 15;

/// Badan `POST /auth/masuk`.
#[derive(Debug, Deserialize)]
pub struct MasukRequest {
    /// Akun peran, contoh `sopir-02`.
    pub akun: String,
    /// PIN 6 digit.
    pub pin: String,
}

/// Badan `POST /auth/refresh`.
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    /// Refresh token (`<id>.<rahasia>`).
    pub refresh_token: String,
}

/// Badan `POST /gudang/masuk`.
#[derive(Debug, Deserialize)]
pub struct GudangMasukRequest {
    /// Kode operator, contoh `OPR-SORTIR-1`.
    pub operator: String,
    /// PIN 6 digit.
    pub pin: String,
}

/// Respons `TokenResponse`.
#[derive(Debug, Serialize)]
pub struct TokenResponse {
    /// JWT akses 15 menit.
    pub token: String,
    /// Refresh token 7 hari.
    pub refresh_token: String,
    /// Waktu token akses kedaluwarsa.
    pub berlaku_sampai: DateTime<Utc>,
    /// Peran pengguna.
    pub peran: String,
    /// Kode trip hari ini; hanya untuk peran sopir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trip_hari_ini: Option<Vec<String>>,
}

/// Respons `TokenShiftResponse`.
#[derive(Debug, Serialize)]
pub struct TokenShiftResponse {
    /// Token shift 12 jam.
    pub token: String,
    /// Waktu token shift kedaluwarsa.
    pub berlaku_sampai: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct Akun {
    id: Uuid,
    akun: String,
    peran: String,
    pin_hash: String,
    terkunci_sampai: Option<DateTime<Utc>>,
    aktif: bool,
}

/// Periksa akun + PIN: 422 bila format salah, 401 bila salah/akun tak ada, 429 bila terkunci.
async fn periksa_pin(
    pool: &PgPool,
    akun: &str,
    pin_masuk: &str,
    perangkat: Option<&str>,
) -> AppResult<Akun> {
    if akun.trim().is_empty() {
        return Err(AppError::Unprocessable("akun wajib diisi".into()));
    }
    if !pin::format_valid(pin_masuk) {
        return Err(AppError::Unprocessable("PIN harus 6 digit angka".into()));
    }
    let baris = sqlx::query_as::<_, Akun>(
        "SELECT id, akun, peran, pin_hash, terkunci_sampai, aktif FROM pengguna WHERE akun = $1",
    )
    .bind(akun.trim())
    .fetch_optional(pool)
    .await?;

    let mut conn = pool.acquire().await?;
    let Some(a) = baris else {
        pin::verifikasi_palsu(pin_masuk);
        audit::catat(
            &mut conn,
            akun,
            "masuk_gagal",
            perangkat,
            json!({"alasan": "akun_tidak_ada"}),
        )
        .await?;
        return Err(AppError::Unauthorized);
    };
    if !a.aktif {
        pin::verifikasi_palsu(pin_masuk);
        audit::catat(
            &mut conn,
            &a.akun,
            "masuk_gagal",
            perangkat,
            json!({"alasan": "nonaktif"}),
        )
        .await?;
        return Err(AppError::Unauthorized);
    }
    if let Some(sampai) = a.terkunci_sampai {
        if sampai > Utc::now() {
            audit::catat(
                &mut conn,
                &a.akun,
                "masuk_ditolak_terkunci",
                perangkat,
                json!({"terkunci_sampai": sampai}),
            )
            .await?;
            return Err(AppError::TooManyRequests(format!(
                "akun terkunci sampai {}",
                sampai.to_rfc3339()
            )));
        }
    }

    if pin::verifikasi(pin_masuk, &a.pin_hash) {
        sqlx::query("UPDATE pengguna SET gagal_pin = 0, terkunci_sampai = NULL WHERE id = $1")
            .bind(a.id)
            .execute(&mut *conn)
            .await?;
        return Ok(a);
    }

    // Salah PIN: naikkan penghitung secara atomik; ke-3 mengunci 15 menit dan mereset penghitung.
    let terkunci: Option<DateTime<Utc>> = sqlx::query_scalar(
        "UPDATE pengguna SET
            gagal_pin = CASE WHEN gagal_pin + 1 >= $2 THEN 0 ELSE gagal_pin + 1 END,
            terkunci_sampai = CASE WHEN gagal_pin + 1 >= $2
                THEN now() + make_interval(mins => $3) ELSE terkunci_sampai END
         WHERE id = $1
         RETURNING terkunci_sampai",
    )
    .bind(a.id)
    .bind(BATAS_SALAH_PIN)
    .bind(LAMA_KUNCI_MENIT as i32)
    .fetch_one(&mut *conn)
    .await?;
    let baru_terkunci = terkunci.map(|t| t > Utc::now()).unwrap_or(false);
    audit::catat(
        &mut conn,
        &a.akun,
        if baru_terkunci {
            "akun_terkunci"
        } else {
            "masuk_gagal"
        },
        perangkat,
        json!({"alasan": "pin_salah", "terkunci_sampai": terkunci}),
    )
    .await?;
    Err(AppError::Unauthorized)
}

fn acak_hex(byte: usize) -> String {
    let mut buf = vec![0u8; byte];
    OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Terbitkan refresh token baru dalam keluarga `keluarga`; mengembalikan `<id>.<rahasia>`.
async fn simpan_refresh(
    conn: &mut PgConnection,
    keluarga: Uuid,
    pengguna: Uuid,
    perangkat: Option<&str>,
) -> AppResult<String> {
    let id = Uuid::now_v7();
    let rahasia = acak_hex(32);
    sqlx::query(
        "INSERT INTO refresh_token (id, family_id, pengguna_id, token_hash, perangkat_id, expires_at)
         VALUES ($1, $2, $3, encode(digest($4, 'sha256'), 'hex'), $5, $6)",
    )
    .bind(id)
    .bind(keluarga)
    .bind(pengguna)
    .bind(&rahasia)
    .bind(perangkat)
    .bind(Utc::now() + Duration::days(UMUR_REFRESH_HARI))
    .execute(conn)
    .await?;
    Ok(format!("{id}.{rahasia}"))
}

async fn token_response(
    state: &AppState,
    id: Uuid,
    akun: &str,
    peran: &str,
    keluarga: Uuid,
    refresh_token: String,
) -> AppResult<TokenResponse> {
    let (token, klaim) = state.jwt.terbitkan_akses(id, akun, peran, keluarga)?;
    let trip_hari_ini = if Peran::dari_str(peran) == Some(Peran::Sopir) {
        let mut conn = state.pool.acquire().await?;
        Some(crate::trip::kode_trip_hari_ini(&mut conn, akun).await?)
    } else {
        None
    };
    Ok(TokenResponse {
        token,
        refresh_token,
        berlaku_sampai: exp_sebagai_waktu(&klaim),
        peran: peran.to_string(),
        trip_hari_ini,
    })
}

/// `POST /auth/masuk`: akun peran + PIN menjadi JWT akses 15 menit + refresh 7 hari.
pub async fn masuk(
    State(state): State<AppState>,
    perangkat: PerangkatId,
    ApiJson(req): ApiJson<MasukRequest>,
) -> AppResult<Json<TokenResponse>> {
    let a = periksa_pin(&state.pool, &req.akun, &req.pin, perangkat.sebagai_str()).await?;
    let keluarga = Uuid::now_v7();
    let mut tx = state.pool.begin().await?;
    let refresh = simpan_refresh(&mut tx, keluarga, a.id, perangkat.sebagai_str()).await?;
    audit::catat(
        &mut tx,
        &a.akun,
        "masuk",
        perangkat.sebagai_str(),
        json!({"peran": a.peran}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        token_response(&state, a.id, &a.akun, &a.peran, keluarga, refresh).await?,
    ))
}

#[derive(Debug, sqlx::FromRow)]
struct BarisRefresh {
    family_id: Uuid,
    pengguna_id: Uuid,
    cocok: bool,
    expires_at: DateTime<Utc>,
    used_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    akun: String,
    peran: String,
    aktif: bool,
}

/// `POST /auth/refresh`: rotasi refresh; pemakaian ulang mencabut seluruh keluarga pengguna.
pub async fn refresh(
    State(state): State<AppState>,
    perangkat: PerangkatId,
    ApiJson(req): ApiJson<RefreshRequest>,
) -> AppResult<Json<TokenResponse>> {
    let (id_teks, rahasia) = req
        .refresh_token
        .split_once('.')
        .ok_or(AppError::Unauthorized)?;
    let id = Uuid::parse_str(id_teks).map_err(|_| AppError::Unauthorized)?;

    let mut tx = state.pool.begin().await?;
    let baris = sqlx::query_as::<_, BarisRefresh>(
        "SELECT r.family_id, r.pengguna_id,
                r.token_hash = encode(digest($2, 'sha256'), 'hex') AS cocok,
                r.expires_at, r.used_at, r.revoked_at,
                p.akun, p.peran, p.aktif
         FROM refresh_token r JOIN pengguna p ON p.id = r.pengguna_id
         WHERE r.id = $1 FOR UPDATE OF r",
    )
    .bind(id)
    .bind(rahasia)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::Unauthorized)?;

    if !baris.cocok {
        return Err(AppError::Unauthorized);
    }
    if baris.used_at.is_some() || baris.revoked_at.is_some() {
        // Reuse-detection: token lama dipakai lagi, cabut semua keluarga milik pengguna ini.
        sqlx::query(
            "UPDATE refresh_token SET revoked_at = now()
             WHERE pengguna_id = $1 AND revoked_at IS NULL",
        )
        .bind(baris.pengguna_id)
        .execute(&mut *tx)
        .await?;
        audit::catat(
            &mut tx,
            &baris.akun,
            "refresh_dipakai_ulang",
            perangkat.sebagai_str(),
            json!({"keluarga": baris.family_id}),
        )
        .await?;
        tx.commit().await?; // pencabutan harus tetap tersimpan walau responsnya 401
        return Err(AppError::Unauthorized);
    }
    if baris.expires_at <= Utc::now() || !baris.aktif {
        return Err(AppError::Unauthorized);
    }

    sqlx::query("UPDATE refresh_token SET used_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let baru = simpan_refresh(
        &mut tx,
        baris.family_id,
        baris.pengguna_id,
        perangkat.sebagai_str(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        token_response(
            &state,
            baris.pengguna_id,
            &baris.akun,
            &baris.peran,
            baris.family_id,
            baru,
        )
        .await?,
    ))
}

/// `POST /auth/keluar`: hapus keluarga refresh dari sesi ini (atau cabut sesi shift).
pub async fn keluar(State(state): State<AppState>, pengguna: Pengguna) -> AppResult<StatusCode> {
    let mut tx = state.pool.begin().await?;
    if let Some(keluarga) = pengguna.keluarga {
        sqlx::query("DELETE FROM refresh_token WHERE family_id = $1")
            .bind(keluarga)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(jti) = pengguna.shift_jti {
        sqlx::query("UPDATE sesi_shift SET revoked_at = now() WHERE jti = $1")
            .bind(jti)
            .execute(&mut *tx)
            .await?;
    }
    audit::catat(
        &mut tx,
        &pengguna.akun,
        "keluar",
        pengguna.perangkat_id.as_deref(),
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /gudang/masuk`: `X-Device-Key` sah + PIN operator menjadi token shift 12 jam.
pub async fn gudang_masuk(
    State(state): State<AppState>,
    PerangkatGudang(kunci): PerangkatGudang,
    perangkat: PerangkatId,
    ApiJson(req): ApiJson<GudangMasukRequest>,
) -> AppResult<Json<TokenShiftResponse>> {
    let a = periksa_pin(
        &state.pool,
        &req.operator,
        &req.pin,
        perangkat.sebagai_str(),
    )
    .await?;
    if Peran::dari_str(&a.peran) != Some(Peran::AdminGudang) {
        let mut conn = state.pool.acquire().await?;
        audit::catat(
            &mut conn,
            &a.akun,
            "akses_ditolak",
            perangkat.sebagai_str(),
            json!({"alasan": "bukan_operator_gudang", "peran": a.peran}),
        )
        .await?;
        return Err(AppError::Forbidden);
    }
    let (token, klaim) = state.jwt.terbitkan_shift(a.id, &a.akun, &a.peran)?;
    let berlaku_sampai = exp_sebagai_waktu(&klaim);
    let jti = Uuid::parse_str(&klaim.jti).map_err(|e| AppError::Internal(e.into()))?;
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO sesi_shift (jti, pengguna_id, kunci_id, perangkat_id, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(jti)
    .bind(a.id)
    .bind(kunci.id)
    .bind(perangkat.sebagai_str())
    .bind(berlaku_sampai)
    .execute(&mut *tx)
    .await?;
    audit::catat(
        &mut tx,
        &a.akun,
        "shift_masuk",
        perangkat.sebagai_str(),
        json!({"kunci": kunci.label}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(TokenShiftResponse {
        token,
        berlaku_sampai,
    }))
}

/// `POST /gudang/keluar`: tutup shift dan cabut token shift.
pub async fn gudang_keluar(State(state): State<AppState>, gudang: Gudang) -> AppResult<StatusCode> {
    let jti = gudang.pengguna.shift_jti.ok_or(AppError::Unauthorized)?; // hanya token shift yang boleh menutup shift
    let mut tx = state.pool.begin().await?;
    sqlx::query("UPDATE sesi_shift SET revoked_at = now() WHERE jti = $1")
        .bind(jti)
        .execute(&mut *tx)
        .await?;
    audit::catat(
        &mut tx,
        &gudang.pengguna.akun,
        "shift_keluar",
        gudang.pengguna.perangkat_id.as_deref(),
        json!({"kunci": gudang.perangkat.label}),
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
