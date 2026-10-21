//! Trip dan surat jalan (LMB-BE-005): susun trip, terbitkan `SJ-YYYYMMDD-NNN`, trip hari ini sopir.
//!
//! Kode trip `TRIP-RUTE-NNN` (UT/SL/KT per rute, penghitung permanen). Surat jalan satu per trip;
//! nomor resmi dari server, nomor sementara `TMP-*` (klien offline) dipetakan saat sinkron.
//! Titik, sopir, dan tanggal trip adalah tambahan opsional pada kontrak (lihat ADR-0004).

use std::collections::HashSet;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::{
    auth::{
        peran::Peran,
        principal::{OperatorGudang, PeranSopir},
    },
    domain::{tarif, AksiHasil, Hasil, Konteks, Zona},
    error::{AppError, AppResult},
    extract::ApiJson,
    fee::{self, FeeJson},
    keranjang::kode_keranjang_valid,
    state::AppState,
    urutan, waktu,
};

/// Rute trip (FSD/20): dipetakan ke kode dua huruf pada `TRIP-RUTE-NNN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Rute {
    /// Rute utara (`UT`, contoh `TRIP-UT-041`).
    Utara,
    /// Rute selatan (`SL`).
    Selatan,
    /// Rute kota (`KT`).
    Kota,
}

impl Rute {
    /// Teks basis data/kontrak.
    pub fn sebagai_str(self) -> &'static str {
        match self {
            Rute::Utara => "UTARA",
            Rute::Selatan => "SELATAN",
            Rute::Kota => "KOTA",
        }
    }

    /// Kode dua huruf pada nomor trip.
    pub fn kode(self) -> &'static str {
        match self {
            Rute::Utara => "UT",
            Rute::Selatan => "SL",
            Rute::Kota => "KT",
        }
    }
}

/// Maksimal titik per trip.
pub const MAKS_TITIK: usize = 20;

/// Titik tujuan pada `BuatTripRequest` (tambahan opsional kontrak).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitikBaru {
    /// Kode titik, contoh `PWN-1`.
    pub id: String,
    /// Zona fee titik ini.
    pub zona: Zona,
    /// Pesanan `PSO-YYYYMMDD-NNN` yang dilayani titik ini (dasar fee, LMB-BE-006).
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub pesanan: Option<String>,
    /// Pembeli yang ditagih fee (invoice mingguan per pembeli), maksimal 64 karakter.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub pembeli: Option<String>,
    /// Keranjang (subset `keranjang` trip) yang diantar ke titik ini.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub keranjang: Option<Vec<String>>,
}

/// Badan `POST /trip`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuatTripRequest {
    /// UUID klien (idempotensi).
    pub uuid: Uuid,
    /// Rute.
    pub rute: Rute,
    /// Kendaraan, contoh `L 1234 AB`.
    pub kendaraan: String,
    /// Kas jalan; bila ada harus Rp300.000 (FSD/20), bila kosong memakai nilai itu.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub kas_jalan: Option<i64>,
    /// Kode keranjang yang dimuat (`KRJ-NNN`, minimal 1).
    pub keranjang: Vec<String>,
    /// Tanggal tugas (bawaan: hari ini WIB).
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub tanggal: Option<NaiveDate>,
    /// Akun sopir yang bertugas (opsional).
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub sopir: Option<String>,
    /// Titik tujuan berurutan (opsional).
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub titik: Option<Vec<TitikBaru>>,
}

/// Badan `POST /trip/{id}/surat-jalan`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuratJalanRequest {
    /// UUID klien (idempotensi).
    pub uuid: Uuid,
    /// Kode trip; hanya dipakai rekaman sinkron (pada endpoint, kode trip ada di path).
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub trip_id: Option<String>,
}

/// Skema `Titik` pada kontrak.
#[derive(Debug, Serialize)]
pub struct TitikJson {
    /// Kode titik.
    pub id: String,
    /// Urutan kunjungan mulai 1.
    pub urutan: i32,
    /// Zona fee.
    pub zona: String,
    /// Waktu tiba (WIB) atau null.
    pub tiba_pada: Option<DateTime<FixedOffset>>,
    /// Waktu berangkat (WIB) atau null.
    pub berangkat_pada: Option<DateTime<FixedOffset>>,
}

/// Skema `Trip` pada kontrak (+ `tanggal` dan `sopir` opsional).
#[derive(Debug, Serialize)]
pub struct TripJson {
    /// Kode trip.
    pub kode: String,
    /// Rute.
    pub rute: String,
    /// Kendaraan.
    pub kendaraan: String,
    /// Kas jalan (rupiah).
    pub kas_jalan: i64,
    /// Status trip.
    pub status: String,
    /// Tanggal tugas.
    pub tanggal: NaiveDate,
    /// Akun sopir (bila ditugaskan).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sopir: Option<String>,
    /// Titik berurutan.
    pub titik: Vec<TitikJson>,
}

/// Satu keranjang pada surat jalan.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct KeranjangSj {
    /// Kode keranjang.
    pub kode: String,
    /// Grade isi.
    pub grade: String,
    /// Berat (gram).
    pub berat_g: i64,
}

/// Skema `SuratJalan` pada kontrak.
#[derive(Debug, Serialize)]
pub struct SuratJalanJson {
    /// Nomor resmi `SJ-YYYYMMDD-NNN`.
    pub nomor: String,
    /// Kode trip.
    pub trip: String,
    /// Isi keranjang.
    pub keranjang: Vec<KeranjangSj>,
    /// Nomor `TMP-*` yang dipetakan (cetak ulang tersedia bila ada).
    pub nomor_sementara: Option<String>,
    /// Total fee seluruh pesanan pada surat jalan (rupiah).
    pub fee_total: i64,
    /// Fee per pesanan (dihitung server).
    pub rincian_fee: Vec<FeeJson>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct TripRow {
    /// ID internal.
    pub id: Uuid,
    /// Kode trip.
    pub kode: String,
    /// Rute (teks).
    pub rute: String,
    /// Kendaraan.
    pub kendaraan: String,
    /// Kas jalan.
    pub kas_jalan: i64,
    /// Status.
    pub status: String,
    /// Tanggal tugas.
    pub tanggal: NaiveDate,
    /// Akun sopir.
    pub sopir_akun: Option<String>,
}

/// Kolom [`TripRow`].
pub const KOLOM_TRIP: &str = "id, kode, rute, kendaraan, kas_jalan, status, tanggal, sopir_akun";

/// Cari trip lewat kode; `kunci` memakai `FOR UPDATE`.
pub async fn cari_trip(
    conn: &mut PgConnection,
    kode: &str,
    kunci: bool,
) -> AppResult<Option<TripRow>> {
    let sql = format!(
        "SELECT {KOLOM_TRIP} FROM trip WHERE kode = $1{}",
        if kunci { " FOR UPDATE" } else { "" }
    );
    Ok(sqlx::query_as::<_, TripRow>(&sql)
        .bind(kode)
        .fetch_optional(conn)
        .await?)
}

#[derive(Debug, sqlx::FromRow)]
struct TitikRow {
    titik_id: String,
    urutan: i32,
    zona: String,
    tiba_at: Option<DateTime<Utc>>,
    berangkat_at: Option<DateTime<Utc>>,
}

/// Susun [`TripJson`] lengkap dengan titik berurutan.
pub async fn trip_json(conn: &mut PgConnection, trip: TripRow) -> AppResult<TripJson> {
    let baris: Vec<TitikRow> = sqlx::query_as(
        "SELECT titik_id, urutan, zona, tiba_at, berangkat_at
         FROM trip_titik WHERE trip_id = $1 ORDER BY urutan",
    )
    .bind(trip.id)
    .fetch_all(&mut *conn)
    .await?;
    let wib = waktu::wib();
    Ok(TripJson {
        kode: trip.kode,
        rute: trip.rute,
        kendaraan: trip.kendaraan,
        kas_jalan: trip.kas_jalan,
        status: trip.status,
        tanggal: trip.tanggal,
        sopir: trip.sopir_akun,
        titik: baris
            .into_iter()
            .map(|t| TitikJson {
                id: t.titik_id,
                urutan: t.urutan,
                zona: t.zona,
                tiba_pada: t.tiba_at.map(|w| w.with_timezone(&wib)),
                berangkat_pada: t.berangkat_at.map(|w| w.with_timezone(&wib)),
            })
            .collect(),
    })
}

fn validasi_trip(req: &BuatTripRequest) -> AppResult<()> {
    let kendaraan = req.kendaraan.trim();
    if kendaraan.is_empty() || kendaraan.len() > 64 {
        return Err(AppError::Unprocessable(
            "kendaraan wajib diisi (maksimal 64 karakter)".into(),
        ));
    }
    if let Some(kas) = req.kas_jalan {
        if kas != tarif::KAS_JALAN_PER_TRIP {
            return Err(AppError::Unprocessable(format!(
                "kas_jalan harus Rp{} per trip",
                tarif::KAS_JALAN_PER_TRIP
            )));
        }
    }
    if req.keranjang.is_empty() {
        return Err(AppError::Unprocessable(
            "trip minimal memuat 1 keranjang".into(),
        ));
    }
    let mut dilihat = HashSet::new();
    for kode in &req.keranjang {
        if !kode_keranjang_valid(kode) {
            return Err(AppError::Unprocessable(format!(
                "kode keranjang {kode} tidak sah"
            )));
        }
        if !dilihat.insert(kode.as_str()) {
            return Err(AppError::Unprocessable(format!(
                "keranjang {kode} dobel dalam satu trip"
            )));
        }
    }
    if let Some(titik) = &req.titik {
        if titik.len() > MAKS_TITIK {
            return Err(AppError::Unprocessable(format!(
                "titik maksimal {MAKS_TITIK} per trip"
            )));
        }
        let mut id_titik = HashSet::new();
        let mut ditetapkan = HashSet::new();
        let mut pesanan_dilihat = HashSet::new();
        for t in titik {
            let id = t.id.trim();
            if id.is_empty() || id.len() > 32 {
                return Err(AppError::Unprocessable(
                    "id titik wajib diisi (maksimal 32 karakter)".into(),
                ));
            }
            if !id_titik.insert(id) {
                return Err(AppError::Unprocessable(format!("titik {id} dobel")));
            }
            if let Some(p) = &t.pesanan {
                if !pesanan_valid(p) {
                    return Err(AppError::Unprocessable(format!(
                        "pesanan {p} harus berformat PSO-YYYYMMDD-NNN"
                    )));
                }
                if !pesanan_dilihat.insert(p.as_str()) {
                    return Err(AppError::Unprocessable(format!(
                        "pesanan {p} dobel pada beberapa titik"
                    )));
                }
            }
            if t.pembeli
                .as_deref()
                .is_some_and(|p| p.trim().is_empty() || p.len() > 64)
            {
                return Err(AppError::Unprocessable(
                    "pembeli tidak boleh kosong dan maksimal 64 karakter".into(),
                ));
            }
            for k in t.keranjang.iter().flatten() {
                if !dilihat.contains(k.as_str()) {
                    return Err(AppError::Unprocessable(format!(
                        "keranjang {k} pada titik {id} tidak ada dalam daftar keranjang trip"
                    )));
                }
                if !ditetapkan.insert(k.as_str()) {
                    return Err(AppError::Unprocessable(format!(
                        "keranjang {k} ditetapkan ke lebih dari satu titik"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Format pesanan `PSO-YYYYMMDD-NNN`.
pub fn pesanan_valid(p: &str) -> bool {
    p.len() == 16
        && p.starts_with("PSO-")
        && p.as_bytes()[12] == b'-'
        && p[4..12].bytes().all(|b| b.is_ascii_digit())
        && p[13..].bytes().all(|b| b.is_ascii_digit())
}

/// Susun trip di dalam transaksi pemanggil: kunci keranjang, terbitkan kode, tandai lot `DIMUAT`.
pub async fn buat_trip_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    req: &BuatTripRequest,
) -> AppResult<Hasil<TripJson>> {
    validasi_trip(req)?;
    let ada: Option<String> = sqlx::query_scalar("SELECT kode FROM trip WHERE uuid_klien = $1")
        .bind(req.uuid)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(kode) = ada {
        return Ok(Hasil::Duplikat(kode));
    }
    if let Some(sopir) = &req.sopir {
        let peran: Option<(String, bool)> =
            sqlx::query_as("SELECT peran, aktif FROM pengguna WHERE akun = $1")
                .bind(sopir)
                .fetch_optional(&mut *conn)
                .await?;
        match peran {
            Some((p, true)) if Peran::dari_str(&p) == Some(Peran::Sopir) => {}
            _ => {
                return Err(AppError::Unprocessable(format!(
                    "sopir {sopir} bukan akun sopir aktif"
                )))
            }
        }
    }

    // Kunci isi keranjang yang dimuat: harus aktif dan belum di trip lain.
    let isi: Vec<(Uuid, String, Option<Uuid>, Uuid)> = sqlx::query_as(
        "SELECT id, kode, trip_id, lot_id FROM keranjang_isi
         WHERE kode = ANY($1) AND selesai_at IS NULL FOR UPDATE",
    )
    .bind(&req.keranjang)
    .fetch_all(&mut *conn)
    .await?;
    for kode in &req.keranjang {
        match isi.iter().find(|(_, k, _, _)| k == kode) {
            None => {
                return Err(AppError::Unprocessable(format!(
                    "keranjang {kode} tidak terisi; catat lewat /keranjang/batch dahulu"
                )))
            }
            Some((_, _, Some(_), _)) => {
                return Err(AppError::Unprocessable(format!(
                    "keranjang {kode} sudah dimuat ke trip lain"
                )))
            }
            Some(_) => {}
        }
    }

    let nomor =
        urutan::berikutnya_permanen(&mut *conn, &format!("TRIP-{}", req.rute.kode())).await?;
    if nomor > 999 {
        return Err(AppError::Conflict(format!(
            "nomor trip rute {} melewati 999",
            req.rute.sebagai_str()
        )));
    }
    let kode = format!("TRIP-{}-{nomor:03}", req.rute.kode());
    let tanggal = req.tanggal.unwrap_or_else(waktu::hari_ini_wib);

    let mut sp = conn.begin().await?;
    let trip_id: Uuid = match sqlx::query_scalar(
        "INSERT INTO trip (uuid_klien, kode, rute, kendaraan, kas_jalan, tanggal, sopir_akun, dibuat_oleh)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(req.uuid)
    .bind(&kode)
    .bind(req.rute.sebagai_str())
    .bind(req.kendaraan.trim())
    .bind(tarif::KAS_JALAN_PER_TRIP)
    .bind(tanggal)
    .bind(&req.sopir)
    .bind(&ctx.akun)
    .fetch_one(&mut *sp)
    .await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            drop(sp);
            let ada: Option<String> =
                sqlx::query_scalar("SELECT kode FROM trip WHERE uuid_klien = $1")
                    .bind(req.uuid)
                    .fetch_optional(&mut *conn)
                    .await?;
            return ada
                .map(Hasil::Duplikat)
                .ok_or_else(|| AppError::Conflict("kode trip bentrok, coba lagi".into()));
        }
        Err(e) => return Err(e.into()),
    };
    let id_isi: Vec<Uuid> = isi.iter().map(|(id, _, _, _)| *id).collect();
    sqlx::query("UPDATE keranjang_isi SET trip_id = $2 WHERE id = ANY($1)")
        .bind(&id_isi)
        .bind(trip_id)
        .execute(&mut *sp)
        .await?;
    for (i, t) in req.titik.iter().flatten().enumerate() {
        sqlx::query(
            "INSERT INTO trip_titik (trip_id, titik_id, urutan, zona, pesanan, pembeli)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(trip_id)
        .bind(t.id.trim())
        .bind(i as i32 + 1)
        .bind(t.zona.sebagai_str())
        .bind(&t.pesanan)
        .bind(t.pembeli.as_deref().map(str::trim))
        .execute(&mut *sp)
        .await?;
        for k in t.keranjang.iter().flatten() {
            sqlx::query("UPDATE keranjang_isi SET titik_id = $3 WHERE kode = $1 AND trip_id = $2")
                .bind(k)
                .bind(trip_id)
                .bind(t.id.trim())
                .execute(&mut *sp)
                .await?;
        }
    }
    let id_lot: Vec<Uuid> = isi.iter().map(|(_, _, _, l)| *l).collect();
    sqlx::query(
        "UPDATE lot SET status = 'DIMUAT', versi = versi + 1, diperbarui_at = now()
         WHERE id = ANY($1) AND status = 'DISORTIR'",
    )
    .bind(&id_lot)
    .execute(&mut *sp)
    .await?;
    sp.commit().await?;

    let trip = cari_trip(conn, &kode, false)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Hasil::Baru(trip_json(conn, trip).await?))
}

/// `POST /trip`: susun trip (admin gudang).
pub async fn buat(
    State(state): State<AppState>,
    OperatorGudang(gudang): OperatorGudang,
    ApiJson(req): ApiJson<BuatTripRequest>,
) -> AppResult<Response> {
    let ctx = gudang.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = buat_trip_tx(&mut tx, &ctx, &req).await?;
    tx.commit().await?;
    Ok(match hasil {
        Hasil::Baru(trip) => (StatusCode::CREATED, Json(trip)).into_response(),
        Hasil::Duplikat(kode) => (
            StatusCode::OK,
            Json(AksiHasil {
                id: kode,
                duplikat: true,
            }),
        )
            .into_response(),
    })
}

/// `GET /trip-hari-ini`: trip sopir yang bertugas hari ini (WIB) beserta titik berurutan.
pub async fn trip_hari_ini(
    State(state): State<AppState>,
    PeranSopir(pengguna): PeranSopir,
) -> AppResult<Json<serde_json::Value>> {
    let mut conn = state.pool.acquire().await?;
    let sql = format!(
        "SELECT {KOLOM_TRIP} FROM trip WHERE sopir_akun = $1 AND tanggal = $2 ORDER BY kode"
    );
    let baris = sqlx::query_as::<_, TripRow>(&sql)
        .bind(&pengguna.akun)
        .bind(waktu::hari_ini_wib())
        .fetch_all(&mut *conn)
        .await?;
    let mut data = Vec::with_capacity(baris.len());
    for t in baris {
        data.push(trip_json(&mut conn, t).await?);
    }
    Ok(Json(serde_json::json!({ "data": data })))
}

/// Kode trip yang ditugaskan ke sopir hari ini (untuk `TokenResponse.trip_hari_ini`).
pub async fn kode_trip_hari_ini(
    conn: &mut PgConnection,
    akun: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT kode FROM trip WHERE sopir_akun = $1 AND tanggal = $2 ORDER BY kode")
        .bind(akun)
        .bind(waktu::hari_ini_wib())
        .fetch_all(conn)
        .await
}

/// Terbitkan surat jalan untuk trip di dalam transaksi pemanggil.
pub async fn terbit_surat_jalan_tx(
    conn: &mut PgConnection,
    ctx: &Konteks,
    trip_kode: &str,
    uuid: Uuid,
    nomor_sementara: Option<&str>,
) -> AppResult<Hasil<SuratJalanJson>> {
    let ada: Option<String> =
        sqlx::query_scalar("SELECT nomor FROM surat_jalan WHERE uuid_klien = $1")
            .bind(uuid)
            .fetch_optional(&mut *conn)
            .await?;
    if let Some(nomor) = ada {
        return Ok(Hasil::Duplikat(nomor));
    }
    // Klien offline belum tahu kode trip resmi: `trip_id` boleh berupa UUID klien trip itu.
    let trip = match cari_trip(conn, trip_kode, true).await? {
        Some(t) => Some(t),
        None => match Uuid::parse_str(trip_kode) {
            Ok(u) => {
                let kode: Option<String> =
                    sqlx::query_scalar("SELECT kode FROM trip WHERE uuid_klien = $1")
                        .bind(u)
                        .fetch_optional(&mut *conn)
                        .await?;
                match kode {
                    Some(k) => cari_trip(conn, &k, true).await?,
                    None => None,
                }
            }
            Err(_) => None,
        },
    }
    .ok_or(AppError::NotFound)?;
    let sudah: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM surat_jalan WHERE trip_id = $1)")
            .bind(trip.id)
            .fetch_one(&mut *conn)
            .await?;
    if sudah {
        return Err(AppError::Conflict(
            "konflik versi, surat jalan untuk trip ini sudah terbit".into(),
        ));
    }
    let tanggal = waktu::hari_ini_wib();
    let urut = urutan::berikutnya(&mut *conn, "SJ", tanggal).await?;
    let nomor = urutan::format_nomor("SJ", tanggal, urut);
    let mut sp = conn.begin().await?;
    let sj_id: Uuid = match sqlx::query_scalar(
        "INSERT INTO surat_jalan (uuid_klien, trip_id, nomor, nomor_sementara, diterbitkan_oleh)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(uuid)
    .bind(trip.id)
    .bind(&nomor)
    .bind(nomor_sementara)
    .bind(&ctx.akun)
    .fetch_one(&mut *sp)
    .await
    {
        Ok(id) => {
            sp.commit().await?;
            id
        }
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::Conflict(
                "nomor sementara sudah dipakai surat jalan lain".into(),
            ));
        }
        Err(e) => return Err(e.into()),
    };
    let rincian_fee = fee::catat_fee_surat_jalan(conn, sj_id, trip.id, tanggal).await?;
    let fee_total = rincian_fee.iter().map(|f| f.nominal).sum();
    let keranjang = sqlx::query_as::<_, KeranjangSj>(
        "SELECT kode, grade, berat_g FROM keranjang_isi WHERE trip_id = $1 ORDER BY kode",
    )
    .bind(trip.id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Hasil::Baru(SuratJalanJson {
        nomor,
        trip: trip.kode,
        keranjang,
        nomor_sementara: nomor_sementara.map(str::to_string),
        fee_total,
        rincian_fee,
    }))
}

/// `POST /trip/{id}/surat-jalan`: terbitkan surat jalan `SJ-YYYYMMDD-NNN` (admin gudang).
pub async fn terbit_surat_jalan(
    State(state): State<AppState>,
    OperatorGudang(gudang): OperatorGudang,
    Path(id): Path<String>,
    ApiJson(req): ApiJson<SuratJalanRequest>,
) -> AppResult<Response> {
    if req.trip_id.as_deref().is_some_and(|t| t != id) {
        return Err(AppError::Unprocessable(
            "trip_id pada badan berbeda dengan {id} pada path".into(),
        ));
    }
    let ctx = gudang.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = terbit_surat_jalan_tx(&mut tx, &ctx, &id, req.uuid, None).await?;
    tx.commit().await?;
    Ok(match hasil {
        Hasil::Baru(sj) => (StatusCode::CREATED, Json(sj)).into_response(),
        Hasil::Duplikat(nomor) => (
            StatusCode::OK,
            Json(AksiHasil {
                id: nomor,
                duplikat: true,
            }),
        )
            .into_response(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pesanan() {
        assert!(pesanan_valid("PSO-20261010-001"));
        assert!(!pesanan_valid("PSO-20261010-01"));
        assert!(!pesanan_valid("PSO-2026101a-001"));
        assert!(!pesanan_valid("pso-20261010-001"));
    }

    #[test]
    fn kode_rute() {
        assert_eq!(Rute::Utara.kode(), "UT");
        assert_eq!(Rute::Selatan.kode(), "SL");
        assert_eq!(Rute::Kota.kode(), "KT");
    }
}
