//! Fee logistik per kg (LMB-BE-006, BR-301/BR-302): Rp1.500/kg zona dekat, Rp2.500/kg zona jauh.
//!
//! Fee dihitung server saat surat jalan terbit, per pesanan (titik dengan `pesanan`), dari berat
//! keranjang yang ditetapkan ke titik itu. Semua angka integer: berat gram, uang rupiah.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{domain::Zona, error::AppResult};

/// Bagi dua bilangan bulat tak negatif dengan pembulatan setengah naik ke rupiah terdekat.
pub fn bagi_bulat(pembilang: i64, penyebut: i64) -> i64 {
    (pembilang + penyebut / 2) / penyebut
}

/// Fee satu pesanan: berat (gram) x tarif zona per kg, rupiah integer.
/// Contoh BR-301: 170.000 g zona dekat = 170 kg x Rp1.500 = Rp255.000.
pub fn hitung_fee(berat_g: i64, tarif_per_kg: i64) -> i64 {
    bagi_bulat(berat_g * tarif_per_kg, 1000)
}

/// Skema `Fee` pada kontrak.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct FeeJson {
    /// Pesanan `PSO-YYYYMMDD-NNN`.
    pub pesanan: String,
    /// Berat dikirim (gram).
    pub berat_g: i64,
    /// Zona tarif.
    pub zona: Zona,
    /// Tarif per kg (rupiah).
    pub tarif_per_kg: i64,
    /// Nominal fee (rupiah).
    pub nominal: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct TitikFee {
    pesanan: String,
    pembeli: Option<String>,
    zona: String,
    berat_g: i64,
}

/// Hitung dan simpan fee semua pesanan pada trip untuk surat jalan `surat_jalan_id`.
/// Titik tanpa pesanan atau tanpa keranjang yang ditetapkan tidak menghasilkan fee.
pub async fn catat_fee_surat_jalan(
    conn: &mut PgConnection,
    surat_jalan_id: Uuid,
    trip_id: Uuid,
    tanggal: NaiveDate,
) -> AppResult<Vec<FeeJson>> {
    let titik = sqlx::query_as::<_, TitikFee>(
        "SELECT t.pesanan, t.pembeli, t.zona, COALESCE(SUM(k.berat_g), 0)::bigint AS berat_g
         FROM trip_titik t
         LEFT JOIN keranjang_isi k ON k.trip_id = t.trip_id AND k.titik_id = t.titik_id
         WHERE t.trip_id = $1 AND t.pesanan IS NOT NULL
         GROUP BY t.id, t.urutan, t.pesanan, t.pembeli, t.zona
         ORDER BY t.urutan",
    )
    .bind(trip_id)
    .fetch_all(&mut *conn)
    .await?;
    // Tarif yang berlaku pada tanggal surat jalan (bisa berubah lewat LMB-BE-008).
    let tarif = crate::tarif::tarif_pada(&mut *conn, tanggal).await?;
    let mut hasil = Vec::new();
    for t in titik.into_iter().filter(|t| t.berat_g > 0) {
        let zona = Zona::dari_str(&t.zona).ok_or_else(|| {
            crate::error::AppError::Internal(anyhow::anyhow!("zona tidak dikenal: {}", t.zona))
        })?;
        let tarif_per_kg = tarif.untuk(zona);
        let nominal = hitung_fee(t.berat_g, tarif_per_kg);
        sqlx::query(
            "INSERT INTO fee (surat_jalan_id, pesanan, pembeli, berat_g, zona, tarif_per_kg, nominal, tanggal)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(surat_jalan_id)
        .bind(&t.pesanan)
        .bind(&t.pembeli)
        .bind(t.berat_g)
        .bind(zona.sebagai_str())
        .bind(tarif_per_kg)
        .bind(nominal)
        .bind(tanggal)
        .execute(&mut *conn)
        .await?;
        hasil.push(FeeJson {
            pesanan: t.pesanan,
            berat_g: t.berat_g,
            zona,
            tarif_per_kg,
            nominal,
        });
    }
    Ok(hasil)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contoh_br301_170_kg_dekat_255_ribu() {
        assert_eq!(hitung_fee(170_000, Zona::Dekat.tarif_per_kg()), 255_000);
        assert_eq!(hitung_fee(170_000, Zona::Jauh.tarif_per_kg()), 425_000);
        assert_eq!(hitung_fee(1_000, Zona::Dekat.tarif_per_kg()), 1_500);
        assert_eq!(hitung_fee(1_000, Zona::Jauh.tarif_per_kg()), 2_500);
    }

    #[test]
    fn pecahan_kg_dibulatkan_setengah_naik_ke_rupiah() {
        // 1.234 g x 1.500 / 1.000 = 1.851 tepat.
        assert_eq!(hitung_fee(1_234, Zona::Dekat.tarif_per_kg()), 1_851);
        // 333 g x 1.500 / 1.000 = 499,5 -> 500.
        assert_eq!(hitung_fee(333, Zona::Dekat.tarif_per_kg()), 500);
        // 1 g x 1.500 / 1.000 = 1,5 -> 2; 0 g -> 0.
        assert_eq!(hitung_fee(1, Zona::Dekat.tarif_per_kg()), 2);
        assert_eq!(hitung_fee(0, Zona::Dekat.tarif_per_kg()), 0);
    }

    #[test]
    fn tanpa_luapan_pada_berat_besar() {
        // 100 ton = 100.000.000 g, tarif jauh: Rp250.000.000.
        assert_eq!(
            hitung_fee(100_000_000, Zona::Jauh.tarif_per_kg()),
            250_000_000
        );
    }
}
