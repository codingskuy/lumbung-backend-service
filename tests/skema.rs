//! Uji skema akhir: berat selalu gram integer (tidak ada NUMERIC kg), uang integer.

mod common;

use sqlx::PgPool;

#[sqlx::test]
async fn tidak_ada_kolom_numeric_atau_satuan_kg_di_skema_akhir(pool: PgPool) {
    // Migrasi 0001 (templat) tidak diedit karena immutable; 0004/0005 membuang tabel templatnya.
    let numeric: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text FROM information_schema.columns
         WHERE table_schema = 'public' AND data_type IN ('numeric', 'real', 'double precision')
           AND NOT (table_name = 'titik_event' AND column_name IN ('lat', 'lng'))
         ORDER BY 1, 2",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(numeric.is_empty(), "kolom desimal tersisa: {numeric:?}");

    let kg: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text FROM information_schema.columns
         WHERE table_schema = 'public' AND column_name LIKE '%\\_kg' AND column_name <> 'tarif_per_kg' ORDER BY 1, 2",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    // `tarif_per_kg` adalah rupiah per kg (harga), bukan satuan berat.
    assert!(kg.is_empty(), "kolom bersatuan kg tersisa: {kg:?}");
}
