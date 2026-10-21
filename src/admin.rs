//! Endpoint baca web admin dan profil publik (temuan Schemathesis, LMB-BE-TEST-SCHEMATHESIS).
//!
//! Hanya baca: `GET /admin/rekap-harian`, `/admin/mitra`, `/admin/insiden`, `/publik/profil`.
//! Endpoint tulis admin selain papan harga dan invoice lunas sengaja belum ada (ADR-0006, hak tulis
//! admin minimum); `kontrak-saya`, `rencana-setoran`, dan `POST /admin/mitra/{id}/status` menjawab
//! 501 terdokumentasi sampai tiketnya ada (entitas kontrak musim belum didefinisikan).

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::Serialize;
use serde_json::json;

use crate::{
    auth::principal::{AkunAdmin, PeranPetani},
    domain::Komoditas,
    error::{AppError, AppResult},
    papan_harga::CACHE_PUBLIK,
    state::AppState,
    waktu,
};

/// `GET /admin/rekap-harian?tanggal=YYYY-MM-DD`: kg masuk per komoditas (gram), fee tertagih, susut.
pub async fn rekap_harian(
    State(state): State<AppState>,
    _akun: AkunAdmin,
    Query(parameter): Query<HashMap<String, String>>,
) -> AppResult<Json<serde_json::Value>> {
    let tanggal: NaiveDate = parameter
        .get("tanggal")
        .ok_or_else(|| AppError::Unprocessable("parameter tanggal wajib (YYYY-MM-DD)".into()))?
        .parse()
        .map_err(|_| AppError::Unprocessable("tanggal harus berformat YYYY-MM-DD".into()))?;

    let per_komoditas: Vec<(String, i64)> = sqlx::query_as(
        "SELECT komoditas, SUM(berat_kotor_g)::bigint FROM lot
         WHERE (dibuat_at AT TIME ZONE 'Asia/Jakarta')::date = $1 GROUP BY komoditas",
    )
    .bind(tanggal)
    .fetch_all(&state.pool)
    .await?;
    let masuk: Vec<_> = Komoditas::SEMUA
        .iter()
        .map(|k| {
            let berat = per_komoditas
                .iter()
                .find(|(nama, _)| nama == k.sebagai_str())
                .map_or(0, |(_, g)| *g);
            json!({"komoditas": k.sebagai_str(), "berat_g": berat})
        })
        .collect();

    // Susut = (kotor - bersih) / kotor atas lot yang sudah disortir pada hari itu.
    let (kotor, bersih): (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(SUM(l.berat_kotor_g), 0)::bigint, COALESCE(SUM(t.berat_g), 0)::bigint
         FROM lot l JOIN timbangan t ON t.lot_id = l.id AND t.jenis = 'BERSIH'
         WHERE (l.dibuat_at AT TIME ZONE 'Asia/Jakarta')::date = $1",
    )
    .bind(tanggal)
    .fetch_one(&state.pool)
    .await?;
    let susut_persen = if kotor > 0 {
        (kotor - bersih) as f64 * 100.0 / kotor as f64
    } else {
        0.0
    };
    let fee_tertagih: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(nominal), 0)::bigint FROM fee WHERE tanggal = $1")
            .bind(tanggal)
            .fetch_one(&state.pool)
            .await?;
    Ok(Json(json!({
        "tanggal": tanggal,
        "masuk": masuk,
        "fee_tertagih": fee_tertagih,
        "susut_persen": susut_persen,
    })))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
struct Mitra {
    kode: String,
    nama: String,
    status: String,
}

/// `GET /admin/mitra`: daftar petani mitra dan status kontrak (baca saja).
pub async fn mitra(
    State(state): State<AppState>,
    _akun: AkunAdmin,
) -> AppResult<Json<serde_json::Value>> {
    let data = sqlx::query_as::<_, Mitra>("SELECT kode, nama, status FROM petani ORDER BY kode")
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(json!({ "data": data })))
}

#[derive(Debug, sqlx::FromRow)]
struct InsidenRow {
    kode: String,
    jenis: String,
    catatan: Option<String>,
    dicatat_at: DateTime<Utc>,
}

/// `GET /admin/insiden`: log insiden dan konflik sinkron, terbaru lebih dulu (maksimal 500).
pub async fn insiden(
    State(state): State<AppState>,
    _akun: AkunAdmin,
) -> AppResult<Json<serde_json::Value>> {
    let baris = sqlx::query_as::<_, InsidenRow>(
        "SELECT kode, jenis, catatan, dicatat_at FROM insiden ORDER BY dicatat_at DESC, kode DESC LIMIT 500",
    )
    .fetch_all(&state.pool)
    .await?;
    let wib = waktu::wib();
    let data: Vec<_> = baris
        .into_iter()
        .map(|r| {
            let waktu: DateTime<FixedOffset> = r.dicatat_at.with_timezone(&wib);
            let mut o = json!({"id": r.kode, "jenis": r.jenis, "waktu": waktu});
            if let Some(c) = r.catatan {
                o["catatan"] = json!(c);
            }
            o
        })
        .collect();
    Ok(Json(json!({ "data": data })))
}

#[derive(Debug, sqlx::FromRow)]
struct ProfilRow {
    nama: String,
    deskripsi: Option<String>,
    kontak_kemitraan: Option<String>,
}

/// `GET /publik/profil`: profil Lumbung dari tabel `profil_publik`, tanpa login, cache 15 menit.
/// Deskripsi dan kontak kemitraan opsional (dihilangkan bila kosong).
pub async fn profil(State(state): State<AppState>) -> AppResult<Response> {
    let p = sqlx::query_as::<_, ProfilRow>(
        "SELECT nama, deskripsi, kontak_kemitraan FROM profil_publik WHERE id = 1",
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let mut badan = json!({"nama": p.nama});
    if let Some(d) = p.deskripsi {
        badan["deskripsi"] = json!(d);
    }
    if let Some(k) = p.kontak_kemitraan {
        badan["kontak_kemitraan"] = json!(k);
    }
    let mut res = Json(badan).into_response();
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_PUBLIK),
    );
    Ok(res)
}

fn belum_diimplementasi() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({"error": "belum diimplementasi"})),
    )
        .into_response()
}

/// `GET /kontrak-saya`: peran petani; 501 (entitas kontrak musim belum didefinisikan).
pub async fn kontrak_saya(_petani: PeranPetani) -> Response {
    belum_diimplementasi()
}

/// `POST /rencana-setoran`: peran petani; 501 sampai kontrak musim didefinisikan.
pub async fn rencana_setoran(_petani: PeranPetani) -> Response {
    belum_diimplementasi()
}

/// `POST /admin/mitra/{id}/status`: 501; penulisan admin selain papan dan invoice lunas menunggu tiket.
pub async fn mitra_status(_akun: AkunAdmin) -> Response {
    belum_diimplementasi()
}
