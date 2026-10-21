//! Papan harga mingguan (LMB-BE-007, BR-305/BR-306).
//!
//! Admin gudang menerbitkan papan tiap Jumat dari 3 harga referensi per komoditas; server
//! menghitung Grade A (median referensi dikurangi 10 persen, dibulatkan ke bawah kelipatan
//! Rp500) dan Grade B (85 persen Grade A, dibulatkan sama). Klien tidak pernah menghitung
//! fee, grade, atau papan: semua angka berasal dari server.

use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    audit,
    auth::principal::{AdminGudangAkses, Pengguna},
    domain::{tarif, Komoditas},
    error::{AppError, AppResult},
    extract::ApiJson,
    state::AppState,
    waktu,
};

/// Cache-Control untuk papan publik: 15 menit (BR-306, kontrak web).
pub const CACHE_PUBLIK: &str = "public, max-age=900";

/// Harga per grade (rupiah per kg).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HargaGrade {
    /// Grade A.
    pub a: i64,
    /// Grade B.
    pub b: i64,
}

/// Fee dua zona (rupiah per kg).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FeeZona {
    /// Zona DEKAT (<15 km).
    pub dekat: i64,
    /// Zona JAUH (15-25 km).
    pub jauh: i64,
}

/// Skema `PapanHarga` pada kontrak.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PapanHarga {
    /// Harga cabai.
    pub cabai: HargaGrade,
    /// Harga bawang.
    pub bawang: HargaGrade,
    /// Harga kangkung.
    pub kangkung: HargaGrade,
    /// Fee logistik per zona.
    pub fee: FeeZona,
    /// Rentang berlaku, contoh `2026-10-06 s.d. 2026-10-12`.
    pub berlaku: String,
}

/// Satu baris referensi pada `TerbitPapanRequest`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Referensi {
    /// Komoditas.
    pub komoditas: Komoditas,
    /// Tepat 3 harga referensi (Pasar Induk, pasar kota, eceran daring), rupiah per kg.
    pub harga: Vec<i64>,
}

/// Badan `POST /admin/papan-harga`. Bidang tak dikenal (mis. `fee`) ditolak 422: klien tidak
/// boleh mengirim angka hitungan server.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerbitPapanRequest {
    /// UUID klien (idempotensi).
    pub uuid: Uuid,
    /// Hari Senin awal minggu berlaku.
    pub berlaku_mulai: NaiveDate,
    /// Referensi 3 harga untuk tiap komoditas pilot.
    pub referensi: Vec<Referensi>,
}

/// Respons terbit papan (`AuditHasil` + penanda duplikat).
#[derive(Debug, Serialize)]
pub struct HasilTerbit {
    /// ID audit `AUD-YYYYMMDD-NNN`.
    pub audit_id: String,
    /// `true` bila UUID klien sama sudah pernah diproses.
    pub duplikat: bool,
}

fn bulatkan_bawah(nilai: i64, kelipatan: i64) -> i64 {
    nilai.div_euclid(kelipatan) * kelipatan
}

/// Hitung harga papan dari 3 referensi: Grade A = lantai500(median x 90%), Grade B = lantai500(A x 85%).
pub fn hitung_harga(referensi: [i64; 3]) -> HargaGrade {
    let mut urut = referensi;
    urut.sort_unstable();
    let median = urut[1];
    let a = bulatkan_bawah(
        median * (100 - tarif::BUFFER_PERSEN) / 100,
        tarif::KELIPATAN_PAPAN,
    );
    let b = bulatkan_bawah(a * tarif::PERSEN_GRADE_B / 100, tarif::KELIPATAN_PAPAN);
    HargaGrade { a, b }
}

fn format_berlaku(mulai: NaiveDate, selesai: NaiveDate) -> String {
    format!("{mulai} s.d. {selesai}")
}

/// Papan yang berlaku pada `tanggal` (Senin-Minggu), atau `None` bila belum terbit.
pub async fn papan_berlaku(pool: &PgPool, tanggal: NaiveDate) -> AppResult<Option<PapanHarga>> {
    let baris: Option<(Uuid, NaiveDate, NaiveDate, i64, i64)> = sqlx::query_as(
        "SELECT id, berlaku_mulai, berlaku_selesai, fee_dekat, fee_jauh FROM papan_harga
         WHERE berlaku_mulai <= $1 AND berlaku_selesai >= $1",
    )
    .bind(tanggal)
    .fetch_optional(pool)
    .await?;
    let Some((id, mulai, selesai, fee_dekat, fee_jauh)) = baris else {
        return Ok(None);
    };
    let item: Vec<(String, String, i64)> =
        sqlx::query_as("SELECT komoditas, grade, harga FROM papan_harga_item WHERE papan_id = $1")
            .bind(id)
            .fetch_all(pool)
            .await?;
    let cari = |k: Komoditas, grade: &str| -> AppResult<i64> {
        item.iter()
            .find(|(kom, g, _)| kom == k.sebagai_str() && g == grade)
            .map(|(_, _, h)| *h)
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("papan {id} tidak lengkap")))
    };
    let harga = |k: Komoditas| -> AppResult<HargaGrade> {
        Ok(HargaGrade {
            a: cari(k, "A")?,
            b: cari(k, "B")?,
        })
    };
    Ok(Some(PapanHarga {
        cabai: harga(Komoditas::Cabai)?,
        bawang: harga(Komoditas::Bawang)?,
        kangkung: harga(Komoditas::Kangkung)?,
        fee: FeeZona {
            dekat: fee_dekat,
            jauh: fee_jauh,
        },
        berlaku: format_berlaku(mulai, selesai),
    }))
}

fn validasi(req: &TerbitPapanRequest) -> AppResult<[[i64; 3]; 3]> {
    if req.berlaku_mulai.weekday() != Weekday::Mon {
        return Err(AppError::Unprocessable(
            "berlaku_mulai harus hari Senin".into(),
        ));
    }
    let mut hasil: [Option<[i64; 3]>; 3] = [None; 3];
    for r in &req.referensi {
        let idx = Komoditas::SEMUA
            .iter()
            .position(|k| *k == r.komoditas)
            .expect("komoditas pilot");
        if hasil[idx].is_some() {
            return Err(AppError::Unprocessable(format!(
                "referensi {} dobel",
                r.komoditas.sebagai_str()
            )));
        }
        let tiga: [i64; 3] = r.harga.clone().try_into().map_err(|_| {
            AppError::Unprocessable(format!(
                "{} butuh tepat 3 harga referensi",
                r.komoditas.sebagai_str()
            ))
        })?;
        if tiga.iter().any(|h| *h <= 0) {
            return Err(AppError::Unprocessable(
                "harga referensi harus lebih dari 0".into(),
            ));
        }
        hasil[idx] = Some(tiga);
    }
    let mut keluar = [[0i64; 3]; 3];
    for (i, h) in hasil.iter().enumerate() {
        keluar[i] = h.ok_or_else(|| {
            AppError::Unprocessable(format!(
                "referensi {} wajib ada",
                Komoditas::SEMUA[i].sebagai_str()
            ))
        })?;
    }
    Ok(keluar)
}

/// `POST /admin/papan-harga`: terbitkan papan mingguan (admin gudang atau kunci layanan), beraudit.
pub async fn terbit(
    State(state): State<AppState>,
    AdminGudangAkses(akun): AdminGudangAkses,
    ApiJson(req): ApiJson<TerbitPapanRequest>,
) -> AppResult<Response> {
    let referensi = validasi(&req)?;

    let mut tx = state.pool.begin().await?;
    // Idempotensi: UUID klien yang sama mengembalikan hasil pertama.
    let ada: Option<String> =
        sqlx::query_scalar("SELECT audit_id FROM papan_harga WHERE uuid_klien = $1")
            .bind(req.uuid)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(audit_id) = ada {
        return Ok((
            StatusCode::OK,
            Json(HasilTerbit {
                audit_id,
                duplikat: true,
            }),
        )
            .into_response());
    }

    let selesai = req.berlaku_mulai + Duration::days(6);
    let tarif_papan = crate::tarif::tarif_pada(&mut tx, req.berlaku_mulai).await?;
    let lembar = json!(req
        .referensi
        .iter()
        .map(|r| json!({"komoditas": r.komoditas.sebagai_str(), "harga": r.harga}))
        .collect::<Vec<_>>());
    let audit_id = audit::catat_beraudit(
        &mut tx,
        &akun.aktor(),
        "papan_harga_terbit",
        akun.perangkat_id(),
        json!({"uuid": req.uuid, "berlaku_mulai": req.berlaku_mulai}),
    )
    .await?;
    let papan_id: Uuid = match sqlx::query_scalar(
        "INSERT INTO papan_harga
            (uuid_klien, berlaku_mulai, berlaku_selesai, fee_dekat, fee_jauh, referensi, audit_id, diterbitkan_oleh)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(req.uuid)
    .bind(req.berlaku_mulai)
    .bind(selesai)
    .bind(tarif_papan.dekat)
    .bind(tarif_papan.jauh)
    .bind(lembar)
    .bind(&audit_id)
    .bind(akun.aktor())
    .fetch_one(&mut *tx)
    .await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::Conflict(
                "papan harga untuk minggu itu sudah terbit".into(),
            ));
        }
        Err(e) => return Err(e.into()),
    };
    for (i, komoditas) in Komoditas::SEMUA.iter().enumerate() {
        let harga = hitung_harga(referensi[i]);
        for (grade, nilai) in [("A", harga.a), ("B", harga.b)] {
            sqlx::query(
                "INSERT INTO papan_harga_item (papan_id, komoditas, grade, harga) VALUES ($1, $2, $3, $4)",
            )
            .bind(papan_id)
            .bind(komoditas.sebagai_str())
            .bind(grade)
            .bind(nilai)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(HasilTerbit {
            audit_id,
            duplikat: false,
        }),
    )
        .into_response())
}

fn dengan_cache(papan: PapanHarga, cache: &'static str) -> Response {
    let mut res = Json(papan).into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}

/// `GET /publik/papan-harga`: tanpa login, `Cache-Control: public, max-age=900`.
pub async fn publik(State(state): State<AppState>) -> AppResult<Response> {
    let papan = papan_berlaku(&state.pool, waktu::hari_ini_wib())
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(dengan_cache(papan, CACHE_PUBLIK))
}

/// `GET /papan-harga`: papan minggu berjalan untuk petani (baca saja, perlu login).
pub async fn petani(State(state): State<AppState>, _pengguna: Pengguna) -> AppResult<Response> {
    let papan = papan_berlaku(&state.pool, waktu::hari_ini_wib())
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(dengan_cache(papan, "private, max-age=900"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contoh_papan_6_sampai_12_okt_2026() {
        // Referensi disusun agar median x 90% mengulang contoh BR-305.
        assert_eq!(
            hitung_harga([35_000, 36_000, 35_000]),
            HargaGrade {
                a: 31_500,
                b: 26_500
            }
        );
        assert_eq!(
            hitung_harga([31_200, 30_000, 33_000]),
            HargaGrade {
                a: 28_000,
                b: 23_500
            }
        );
        assert_eq!(
            hitung_harga([7_000, 7_500, 6_800]),
            HargaGrade { a: 6_000, b: 5_000 }
        );
    }

    #[test]
    fn median_bukan_rata_rata_dan_urutan_bebas() {
        // Pencilan tinggi tidak menggeser median.
        assert_eq!(
            hitung_harga([30_000, 100_000, 30_000]),
            hitung_harga([30_000, 30_000, 30_000])
        );
        assert_eq!(hitung_harga([1, 3, 2]), hitung_harga([3, 2, 1]));
    }

    #[test]
    fn pembulatan_ke_bawah_kelipatan_500() {
        // 10.000 x 0,9 = 9.000 tepat (kelipatan 500 tidak dibulatkan turun lagi).
        assert_eq!(hitung_harga([10_000; 3]).a, 9_000);
        // 10.100 x 0,9 = 9.090 -> 9.000 (turun, bukan naik ke 9.500).
        assert_eq!(hitung_harga([10_100; 3]).a, 9_000);
        // 10.555 x 0,9 = 9.499,5 -> 9.499 (integer) -> 9.000.
        assert_eq!(hitung_harga([10_555; 3]).a, 9_000);
        // Harga kecil di bawah Rp500 menjadi 0 (tidak negatif).
        assert_eq!(hitung_harga([400; 3]), HargaGrade { a: 0, b: 0 });
    }
}
