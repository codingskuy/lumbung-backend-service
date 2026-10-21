//! Sinkron batch offline (LMB-BE-005): `POST /sinkron/batch`, maksimal 200 rekaman FIFO.
//!
//! Tiap rekaman diproses berurutan dalam savepoint sendiri di dalam satu transaksi, sehingga
//! satu rekaman gagal tidak membatalkan yang lain, dan kegagalan basis data membatalkan seluruh
//! permintaan (klien mengirim ulang dengan aman karena semua rekaman idempoten via UUID).
//! Nomor sementara `TMP-*` dipetakan ke nomor resmi (`L-NNNN`, `SJ-YYYYMMDD-NNN`).
//!
//! Resolusi konflik (FSD/10): gudang menang untuk berat dan grade (rekaman sortir yang bentrok
//! menjadi `KONFLIK` dan data gudang dipertahankan), lapangan menang untuk bukti terima dan posisi.
//! Setiap konflik dicatat sebagai insiden `konflik_sinkron`. Offline lebih dari 3x24 jam mengunci
//! penerbitan surat jalan baru (timbang dan bukti tetap diizinkan).

use axum::{extract::State, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::{
    auth::principal::OperatorGudang,
    domain::{Hasil, Konteks},
    error::{AppError, AppResult},
    extract::ApiJson,
    keranjang::{batch_tx, periksa_grade_c, KeranjangBatchRequest},
    lot::{buat_lot_tx, sortir_tx, BuatLotRequest, SortirRequest},
    sopir::{bukti_terima_tx, catat_konflik, insiden_tx, BuktiTerimaRequest, InsidenRequest},
    state::AppState,
    trip::{buat_trip_tx, terbit_surat_jalan_tx, BuatTripRequest, SuratJalanRequest},
};

/// Maksimal rekaman per request.
pub const MAKS_REKAMAN: usize = 200;
/// Batas offline: lebih dari 3x24 jam mengunci surat jalan baru.
pub const BATAS_OFFLINE_JAM: i64 = 72;

/// Jenis rekaman sinkron (kontrak `SinkronRekaman.jenis`).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Jenis {
    /// `POST /lot`.
    Lot,
    /// `POST /lot/{id}/sortir`.
    Sortir,
    /// `POST /keranjang/batch`.
    Keranjang,
    /// `POST /trip`.
    Trip,
    /// `POST /trip/{id}/surat-jalan` (payload memuat `trip_id`).
    SuratJalan,
    /// `POST /bukti-terima`.
    BuktiTerima,
    /// `POST /insiden`.
    Insiden,
}

/// Satu rekaman antrean outbox.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rekaman {
    /// UUID klien rekaman (harus sama dengan `payload.uuid` bila ada).
    pub uuid: Uuid,
    /// Jenis rekaman.
    pub jenis: Jenis,
    /// Nomor sementara `TMP-*` yang akan dipetakan ke nomor resmi.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub nomor_sementara: Option<String>,
    /// Badan request endpoint asal (harus objek JSON; bukan objek = 422 untuk seluruh request).
    pub payload: serde_json::Map<String, Value>,
    /// Waktu rekaman dibuat di perangkat.
    #[serde(default, deserialize_with = "crate::extract::opsional_tanpa_null")]
    pub dibuat_pada: Option<DateTime<Utc>>,
}

/// Badan `POST /sinkron/batch`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SinkronBatchRequest {
    /// 1 sampai 200 rekaman urut FIFO.
    pub rekaman: Vec<Rekaman>,
}

/// Hasil per rekaman (`SinkronBatchResponse.hasil[]`).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HasilRekaman {
    /// UUID rekaman.
    pub uuid: Uuid,
    /// `DITERIMA`, `DUPLIKAT`, `KONFLIK`, atau `DITOLAK`.
    pub status: &'static str,
    /// Nomor resmi yang memetakan `TMP-*` (atau kode rekaman baru).
    pub nomor_resmi: Option<String>,
    /// Penjelasan untuk `KONFLIK`/`DITOLAK`.
    pub pesan: Option<String>,
}

fn nomor_sementara_valid(n: &str) -> bool {
    n.len() == 12
        && n.starts_with("TMP-")
        && n[4..]
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Sisipkan `uuid` rekaman ke payload dan urai ke tipe request asal.
fn urai<T: serde::de::DeserializeOwned>(rek: &Rekaman) -> AppResult<T> {
    let mut objek = rek.payload.clone();
    match objek.get("uuid") {
        None => {
            objek.insert("uuid".into(), Value::String(rek.uuid.to_string()));
        }
        Some(Value::String(u)) if u == &rek.uuid.to_string() => {}
        Some(_) => {
            return Err(AppError::Unprocessable(
                "payload.uuid berbeda dengan uuid rekaman".into(),
            ))
        }
    }
    serde_json::from_value(Value::Object(objek))
        .map_err(|e| AppError::Unprocessable(format!("payload tidak sah: {e}")))
}

fn diterima(nomor: Option<String>) -> (&'static str, Option<String>, Option<String>) {
    ("DITERIMA", nomor, None)
}

fn hasil_idempoten<T>(
    h: Hasil<T>,
    nomor_baru: impl FnOnce(&T) -> Option<String>,
) -> (&'static str, Option<String>, Option<String>) {
    match h {
        Hasil::Baru(v) => diterima(nomor_baru(&v)),
        Hasil::Duplikat(id) => ("DUPLIKAT", Some(id), None),
    }
}

/// Proses satu rekaman; galat basis data/internal dikembalikan sebagai `Err` (batalkan semua).
async fn proses_rekaman(
    state: &AppState,
    conn: &mut PgConnection,
    ctx: &Konteks,
    rek: &Rekaman,
) -> AppResult<(&'static str, Option<String>, Option<String>)> {
    if let Some(n) = &rek.nomor_sementara {
        if !nomor_sementara_valid(n) {
            return Err(AppError::Unprocessable(format!(
                "nomor_sementara {n} harus berformat TMP-xxxxxxxx (8 heksadesimal)"
            )));
        }
    }
    let nomor_sem = rek.nomor_sementara.as_deref();
    Ok(match rek.jenis {
        Jenis::Lot => {
            let req: BuatLotRequest = urai(rek)?;
            hasil_idempoten(buat_lot_tx(conn, ctx, &req, nomor_sem).await?, |l| {
                Some(l.kode.clone())
            })
        }
        Jenis::Sortir => {
            let req: SortirRequest = urai(rek)?;
            let lot_id = req.lot_id.clone();
            hasil_idempoten(sortir_tx(conn, ctx, &lot_id, &req).await?, |l| {
                Some(l.kode.clone())
            })
        }
        Jenis::Keranjang => {
            let req: KeranjangBatchRequest = urai(rek)?;
            periksa_grade_c(&state.pool, ctx, &req).await?;
            hasil_idempoten(batch_tx(conn, ctx, &req).await?, |_| None)
        }
        Jenis::Trip => {
            let req: BuatTripRequest = urai(rek)?;
            hasil_idempoten(buat_trip_tx(conn, ctx, &req).await?, |t| {
                Some(t.kode.clone())
            })
        }
        Jenis::SuratJalan => {
            let req: SuratJalanRequest = urai(rek)?;
            let trip = req.trip_id.clone().ok_or_else(|| {
                AppError::Unprocessable("payload surat_jalan wajib memuat trip_id".into())
            })?;
            if let Some(dibuat) = rek.dibuat_pada {
                if Utc::now() - dibuat > Duration::hours(BATAS_OFFLINE_JAM) {
                    return Err(AppError::Unprocessable(
                        "offline lebih dari 3x24 jam: penerbitan surat jalan baru dikunci; timbang dan bukti tetap diizinkan".into(),
                    ));
                }
            }
            hasil_idempoten(
                terbit_surat_jalan_tx(conn, ctx, &trip, req.uuid, nomor_sem).await?,
                |s| Some(s.nomor.clone()),
            )
        }
        Jenis::BuktiTerima => {
            let req: BuktiTerimaRequest = urai(rek)?;
            hasil_idempoten(bukti_terima_tx(conn, ctx, &req).await?, |_| None)
        }
        Jenis::Insiden => {
            let req: InsidenRequest = urai(rek)?;
            hasil_idempoten(insiden_tx(conn, ctx, &req).await?, |k| Some(k.clone()))
        }
    })
}

/// Jalankan seluruh rekaman berurutan; kegagalan basis data membatalkan seluruh transaksi.
pub async fn proses_batch(
    state: &AppState,
    conn: &mut PgConnection,
    ctx: &Konteks,
    rekaman: &[Rekaman],
) -> AppResult<Vec<HasilRekaman>> {
    let mut hasil = Vec::with_capacity(rekaman.len());
    for rek in rekaman {
        let mut sp = conn.begin().await?;
        let (status, nomor_resmi, pesan) = match proses_rekaman(state, &mut sp, ctx, rek).await {
            Ok(h) => {
                sp.commit().await?;
                h
            }
            Err(e @ (AppError::Db(_) | AppError::Internal(_))) => return Err(e),
            Err(AppError::Conflict(pesan)) => {
                drop(sp); // batalkan perubahan rekaman ini; data gudang dipertahankan
                let catatan = format!(
                    "konflik sinkron {:?} {}: {pesan}; gudang menang untuk berat dan grade",
                    rek.jenis, rek.uuid
                );
                catat_konflik(conn, ctx, &catatan).await?;
                ("KONFLIK", None, Some(pesan))
            }
            Err(e) => {
                drop(sp);
                ("DITOLAK", None, Some(e.to_string()))
            }
        };
        hasil.push(HasilRekaman {
            uuid: rek.uuid,
            status,
            nomor_resmi,
            pesan,
        });
    }
    Ok(hasil)
}

/// `POST /sinkron/batch`: catch-up setelah offline (admin gudang, bearer + `X-Device-Key`).
pub async fn batch(
    State(state): State<AppState>,
    OperatorGudang(gudang): OperatorGudang,
    ApiJson(req): ApiJson<SinkronBatchRequest>,
) -> AppResult<Json<Value>> {
    if req.rekaman.is_empty() {
        return Err(AppError::Unprocessable("rekaman minimal 1".into()));
    }
    if req.rekaman.len() > MAKS_REKAMAN {
        return Err(AppError::Unprocessable(format!(
            "rekaman maksimal {MAKS_REKAMAN} per request, diterima {}",
            req.rekaman.len()
        )));
    }
    // Format `nomor_sementara` adalah bagian skema: salah format menolak seluruh request (422).
    if let Some(n) = req
        .rekaman
        .iter()
        .filter_map(|r| r.nomor_sementara.as_deref())
        .find(|n| !nomor_sementara_valid(n))
    {
        return Err(AppError::Unprocessable(format!(
            "nomor_sementara {n} harus berformat TMP-xxxxxxxx (8 heksadesimal huruf kecil)"
        )));
    }
    let ctx = gudang.konteks();
    let mut tx = state.pool.begin().await?;
    let hasil = proses_batch(&state, &mut tx, &ctx, &req.rekaman).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "hasil": hasil })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_nomor_sementara() {
        assert!(nomor_sementara_valid("TMP-0a1b2c3d"));
        assert!(!nomor_sementara_valid("TMP-0A1B2C3D"));
        assert!(!nomor_sementara_valid("TMP-0a1b2c3"));
        assert!(!nomor_sementara_valid("tmp-0a1b2c3d"));
        assert!(!nomor_sementara_valid("TMP-0a1b2c3g"));
    }
}
