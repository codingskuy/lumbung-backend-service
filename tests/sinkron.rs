//! Uji LMB-BE-005: `POST /sinkron/batch` (maks 200, FIFO, pemetaan TMP-*, konflik, kunci offline).

mod common;

use axum::{http::StatusCode, Router};
use chrono::{Duration, Utc};
use common::*;
use lumbung_backend_service::{auth::peran::Peran, waktu};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

struct Uji {
    pool: PgPool,
    app: Router,
    sesi: SesiGudang,
    sopir: String,
}

async fn siapkan(pool: PgPool) -> Uji {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let sesi = masuk_gudang(&app, &pool).await;
    buat_petani(&pool, "PTN-007").await;
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let sopir = bearer(&masuk(&app, "sopir-02").await.0);
    Uji {
        pool,
        app,
        sesi,
        sopir,
    }
}

/// Rekaman sinkron dengan UUID baru; `payload` tanpa `uuid` (diisi dari rekaman).
fn rek(jenis: &str, tmp: Option<&str>, payload: Value) -> (Uuid, Value) {
    rek_umur(jenis, tmp, payload, Duration::minutes(5))
}

fn rek_umur(jenis: &str, tmp: Option<&str>, payload: Value, umur: Duration) -> (Uuid, Value) {
    let uuid = Uuid::now_v7();
    let mut r = json!({
        "uuid": uuid, "jenis": jenis, "payload": payload,
        "dibuat_pada": (Utc::now() - umur).to_rfc3339(),
    });
    if let Some(t) = tmp {
        r["nomor_sementara"] = json!(t);
    }
    (uuid, r)
}

impl Uji {
    async fn sinkron(&self, rekaman: Vec<Value>) -> Resp {
        kirim(
            &self.app,
            "POST",
            "/v1/sinkron/batch",
            &self.sesi.h(),
            Some(json!({"rekaman": rekaman})),
        )
        .await
    }

    async fn gudang(&self, uri: &str, badan: Value) -> Resp {
        kirim(&self.app, "POST", uri, &self.sesi.h(), Some(badan)).await
    }

    async fn hitung(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.pool).await.unwrap()
    }

    /// Daftar (status, nomor_resmi) dari respons.
    fn ringkas(r: &Resp) -> Vec<(String, Option<String>)> {
        r.json["hasil"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| {
                (
                    h["status"].as_str().unwrap().to_string(),
                    h["nomor_resmi"].as_str().map(str::to_string),
                )
            })
            .collect()
    }
}

fn lot_payload(kotor: i64) -> Value {
    json!({"kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": "2026-10-06", "berat_kotor_g": kotor})
}

fn tanggal_sj() -> String {
    waktu::hari_ini_wib().format("%Y%m%d").to_string()
}

#[sqlx::test]
async fn rantai_offline_memetakan_tmp_ke_nomor_resmi_dan_aman_dikirim_ulang(pool: PgPool) {
    let u = siapkan(pool).await;
    let (u_lot, lot) = rek("lot", Some("TMP-aaaaaaaa"), lot_payload(1_000_000));
    let (_, sortir) = rek(
        "sortir",
        None,
        json!({"lot_id": "TMP-aaaaaaaa", "grade_a_g": 1_000_000, "grade_b_g": 0, "grade_c_g": 0}),
    );
    let (_, keranjang) = rek(
        "keranjang",
        None,
        json!({"keranjang": [
            {"kode": "KRJ-001", "lot_id": "TMP-aaaaaaaa", "grade": "A", "berat_g": 10_000},
            {"kode": "KRJ-002", "lot_id": "TMP-aaaaaaaa", "grade": "A", "berat_g": 10_000},
        ]}),
    );
    let (u_trip, trip) = rek(
        "trip",
        None,
        json!({"rute": "UTARA", "kendaraan": "L 1", "keranjang": ["KRJ-001", "KRJ-002"]}),
    );
    // Klien offline merujuk trip lewat UUID kliennya (kode resmi belum diketahui).
    let (_, sj) = rek(
        "surat_jalan",
        Some("TMP-bbbbbbbb"),
        json!({"trip_id": u_trip.to_string()}),
    );
    let _ = (u_lot, &lot);
    let batch = vec![lot, sortir, keranjang, trip, sj];

    let r = u.sinkron(batch.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(
        Uji::ringkas(&r),
        vec![
            ("DITERIMA".to_string(), Some("L-0001".to_string())),
            ("DITERIMA".to_string(), Some("L-0001".to_string())),
            ("DITERIMA".to_string(), None),
            ("DITERIMA".to_string(), Some("TRIP-UT-001".to_string())),
            (
                "DITERIMA".to_string(),
                Some(format!("SJ-{}-001", tanggal_sj()))
            ),
        ],
        "{}",
        r.json
    );
    // Urutan hasil sama dengan urutan masukan (FIFO).
    for (i, rekaman) in batch.iter().enumerate() {
        assert_eq!(r.json["hasil"][i]["uuid"], rekaman["uuid"]);
    }
    // Pemetaan tersimpan: cetak ulang tersedia karena nomor berubah.
    let tmp_lot: String =
        sqlx::query_scalar("SELECT nomor_sementara FROM lot WHERE kode = 'L-0001'")
            .fetch_one(&u.pool)
            .await
            .unwrap();
    assert_eq!(tmp_lot, "TMP-aaaaaaaa");
    let tmp_sj: String = sqlx::query_scalar("SELECT nomor_sementara FROM surat_jalan")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(tmp_sj, "TMP-bbbbbbbb");

    // Kirim ulang seluruh batch: semuanya DUPLIKAT, tidak ada rekaman ganda.
    let r = u.sinkron(batch).await;
    assert!(
        Uji::ringkas(&r).iter().all(|(s, _)| s == "DUPLIKAT"),
        "{}",
        r.json
    );
    assert_eq!(u.hitung("SELECT count(*) FROM lot").await, 1);
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_isi").await, 2);
    assert_eq!(u.hitung("SELECT count(*) FROM trip").await, 1);
    assert_eq!(u.hitung("SELECT count(*) FROM surat_jalan").await, 1);

    // Path endpoint biasa mengenali TMP-* yang sudah dipetakan (sortir ulang = 409 karena sudah disortir).
    let r = u
        .gudang("/v1/lot/TMP-aaaaaaaa/sortir", json!({"uuid": Uuid::now_v7(), "lot_id": "TMP-aaaaaaaa", "grade_a_g": 1, "grade_b_g": 0, "grade_c_g": 0}))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn urutan_fifo_dipertahankan_dan_rekaman_gagal_tidak_membatalkan_yang_lain(pool: PgPool) {
    let u = siapkan(pool).await;
    // Sortir lebih dulu dari lotnya: ditolak (404), lot di belakangnya tetap masuk; urutan tidak ditata ulang.
    let (_, sortir) = rek(
        "sortir",
        None,
        json!({"lot_id": "TMP-cccccccc", "grade_a_g": 100_000, "grade_b_g": 0, "grade_c_g": 0}),
    );
    let (_, lot) = rek("lot", Some("TMP-cccccccc"), lot_payload(100_000));
    let r = u.sinkron(vec![sortir.clone(), lot]).await;
    let s = Uji::ringkas(&r);
    assert_eq!(s[0].0, "DITOLAK", "{}", r.json);
    assert!(r.json["hasil"][0]["pesan"]
        .as_str()
        .unwrap()
        .contains("tidak ditemukan"));
    assert_eq!(s[1], ("DITERIMA".to_string(), Some("L-0001".to_string())));
    // Kirim ulang antrean (sekarang lot ada): sortir kini diterima.
    let r = u.sinkron(vec![sortir]).await;
    assert_eq!(Uji::ringkas(&r)[0].0, "DITERIMA", "{}", r.json);

    // Batch keranjang yang gagal sebagian dibatalkan utuh (savepoint), rekaman sebelumnya tetap.
    let (_, ok) = rek("insiden", None, json!({"jenis": "keterlambatan"}));
    let (_, buruk) = rek(
        "keranjang",
        None,
        json!({"keranjang": [
            {"kode": "KRJ-001", "lot_id": "TMP-cccccccc", "grade": "A", "berat_g": 50_000},
            {"kode": "KRJ-002", "lot_id": "TMP-cccccccc", "grade": "A", "berat_g": 60_000},
        ]}),
    );
    let r = u.sinkron(vec![ok, buruk]).await;
    let s = Uji::ringkas(&r);
    assert_eq!(s[0].0, "DITERIMA");
    assert_eq!(s[1].0, "DITOLAK", "{}", r.json);
    assert!(r.json["hasil"][1]["pesan"]
        .as_str()
        .unwrap()
        .contains("stok"));
    assert_eq!(
        u.hitung("SELECT count(*) FROM keranjang_isi").await,
        0,
        "batch gagal tidak meninggalkan baris"
    );
    assert_eq!(
        u.hitung("SELECT count(*) FROM insiden WHERE jenis = 'keterlambatan'")
            .await,
        1
    );
}

#[sqlx::test]
async fn batas_jumlah_rekaman_maks_200(pool: PgPool) {
    let u = siapkan(pool).await;
    assert_eq!(
        u.sinkron(vec![]).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let banyak = |n: usize| {
        (0..n)
            .map(|i| rek("insiden", None, json!({"jenis": format!("uji-{i}")})).1)
            .collect::<Vec<_>>()
    };
    let r = u.sinkron(banyak(201)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        r.json["error"].as_str().unwrap().contains("maksimal 200"),
        "{}",
        r.json
    );
    assert_eq!(
        u.hitung("SELECT count(*) FROM insiden").await,
        0,
        "201 rekaman ditolak seluruhnya"
    );
    let r = u.sinkron(banyak(200)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let s = Uji::ringkas(&r);
    assert_eq!(s.len(), 200);
    assert!(s.iter().all(|(st, _)| st == "DITERIMA"));
    assert_eq!(
        s[0].1.as_deref(),
        Some(format!("INS-{}-001", tanggal_sj()).as_str())
    );
    assert_eq!(
        s[199].1.as_deref(),
        Some(format!("INS-{}-200", tanggal_sj()).as_str())
    );
}

#[sqlx::test]
async fn gudang_menang_untuk_berat_dan_grade_konflik_dicatat(pool: PgPool) {
    let u = siapkan(pool).await;
    // Gudang sudah mengesahkan sortir lewat endpoint langsung.
    let kode = u.gudang("/v1/lot", json!({"uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": "2026-10-06", "berat_kotor_g": 120_000})).await.json["kode"].as_str().unwrap().to_string();
    let r = u.gudang(&format!("/v1/lot/{kode}/sortir"), json!({"uuid": Uuid::now_v7(), "lot_id": kode, "grade_a_g": 100_000, "grade_b_g": 14_000, "grade_c_g": 6_000})).await;
    assert_eq!(r.status, StatusCode::CREATED);

    // Rekaman offline dari perangkat lain dengan angka berbeda bentrok: gudang menang.
    let (_, lawan) = rek(
        "sortir",
        None,
        json!({"lot_id": kode, "grade_a_g": 90_000, "grade_b_g": 20_000, "grade_c_g": 10_000}),
    );
    let r = u.sinkron(vec![lawan]).await;
    assert_eq!(Uji::ringkas(&r)[0].0, "KONFLIK", "{}", r.json);
    assert_eq!(
        r.json["hasil"][0]["pesan"],
        "konflik versi, data gudang lebih baru"
    );
    let (a, b, c): (i64, i64, i64) =
        sqlx::query_as("SELECT grade_a_g, grade_b_g, grade_c_g FROM lot WHERE kode = $1")
            .bind(&kode)
            .fetch_one(&u.pool)
            .await
            .unwrap();
    assert_eq!(
        (a, b, c),
        (100_000, 14_000, 6_000),
        "angka gudang tidak berubah"
    );
    assert_eq!(u.hitung("SELECT count(*) FROM sortir").await, 1);
    // Konflik tercatat di log (insiden konflik_sinkron).
    assert_eq!(u.hitung("SELECT count(*) FROM insiden WHERE jenis = 'konflik_sinkron' AND catatan LIKE '%gudang menang%'").await, 1);
}

#[sqlx::test]
async fn lapangan_menang_untuk_bukti_terima_dan_konflik_dicatat(pool: PgPool) {
    let u = siapkan(pool).await;
    // Siapkan trip dengan titik lewat sinkron sendiri (lot, sortir, keranjang, trip).
    let (_, lot) = rek("lot", Some("TMP-dddddddd"), lot_payload(1_000_000));
    let (_, sortir) = rek(
        "sortir",
        None,
        json!({"lot_id": "TMP-dddddddd", "grade_a_g": 1_000_000, "grade_b_g": 0, "grade_c_g": 0}),
    );
    let (_, krj) = rek(
        "keranjang",
        None,
        json!({"keranjang": [{"kode": "KRJ-001", "lot_id": "TMP-dddddddd", "grade": "A", "berat_g": 10_000}]}),
    );
    let (_, trip) = rek(
        "trip",
        None,
        json!({"rute": "KOTA", "kendaraan": "L 9", "keranjang": ["KRJ-001"], "sopir": "sopir-02", "titik": [{"id": "PWN-1", "zona": "DEKAT", "keranjang": ["KRJ-001"]}]}),
    );
    let r = u.sinkron(vec![lot, sortir, krj, trip]).await;
    assert!(
        Uji::ringkas(&r).iter().all(|(s, _)| s == "DITERIMA"),
        "{}",
        r.json
    );

    let foto = Uuid::now_v7().to_string();
    let (ct, badan) = multipart_foto(Some(&foto), &jpeg_uji(3_000));
    assert_eq!(
        kirim_bytes(
            &u.app,
            "/v1/unggah-foto",
            &[("authorization", &u.sopir)],
            &ct,
            badan
        )
        .await
        .status,
        StatusCode::CREATED
    );

    let bukti = |catatan: &str| {
        rek(
            "bukti_terima",
            None,
            json!({"trip_id": "TRIP-KT-001", "titik_id": "PWN-1", "waktu": "2026-10-10T07:20:00+07:00", "foto_ids": [foto], "catatan": catatan}),
        )
    };
    let r = u.sinkron(vec![bukti("pertama").1]).await;
    assert_eq!(Uji::ringkas(&r)[0].0, "DITERIMA", "{}", r.json);
    let r = u.sinkron(vec![bukti("terbaru dari lapangan").1]).await;
    assert_eq!(
        Uji::ringkas(&r)[0].0,
        "DITERIMA",
        "lapangan menang: bukti baru diterima: {}",
        r.json
    );
    assert_eq!(
        u.hitung("SELECT count(*) FROM bukti_terima").await,
        2,
        "riwayat tersimpan"
    );
    let terbaru: String = sqlx::query_scalar("SELECT catatan FROM bukti_terima WHERE trip_id = (SELECT id FROM trip) ORDER BY dicatat_at DESC LIMIT 1").fetch_one(&u.pool).await.unwrap();
    assert_eq!(terbaru, "terbaru dari lapangan");
    assert_eq!(u.hitung("SELECT count(*) FROM insiden WHERE jenis = 'konflik_sinkron' AND catatan LIKE '%lapangan menang%'").await, 1);
}

#[sqlx::test]
async fn offline_lebih_dari_3x24_jam_mengunci_surat_jalan_tetapi_bukan_timbang_dan_bukti(
    pool: PgPool,
) {
    let u = siapkan(pool).await;
    let empat_hari = Duration::days(4);
    // Siapkan trip (dibuat baru).
    let (_, lot) = rek("lot", Some("TMP-eeeeeeee"), lot_payload(1_000_000));
    let (_, sortir) = rek(
        "sortir",
        None,
        json!({"lot_id": "TMP-eeeeeeee", "grade_a_g": 1_000_000, "grade_b_g": 0, "grade_c_g": 0}),
    );
    let (_, krj) = rek(
        "keranjang",
        None,
        json!({"keranjang": [{"kode": "KRJ-001", "lot_id": "TMP-eeeeeeee", "grade": "A", "berat_g": 10_000}]}),
    );
    let (_, trip) = rek(
        "trip",
        None,
        json!({"rute": "UTARA", "kendaraan": "L 1", "keranjang": ["KRJ-001"], "titik": [{"id": "PWN-1", "zona": "DEKAT"}]}),
    );
    u.sinkron(vec![lot, sortir, krj, trip]).await;

    // Timbang (lot) dan bukti terima dari antrean 4 hari lalu tetap diterima.
    let (_, lot_lama) = rek_umur("lot", Some("TMP-ffffffff"), lot_payload(5_000), empat_hari);
    let foto = Uuid::now_v7().to_string();
    let (ct, badan) = multipart_foto(Some(&foto), &jpeg_uji(3_000));
    kirim_bytes(
        &u.app,
        "/v1/unggah-foto",
        &[("authorization", &u.sopir)],
        &ct,
        badan,
    )
    .await;
    let (_, bukti_lama) = rek_umur(
        "bukti_terima",
        None,
        json!({"trip_id": "TRIP-UT-001", "titik_id": "PWN-1", "waktu": "2026-10-07T07:20:00+07:00", "foto_ids": [foto]}),
        empat_hari,
    );
    // Surat jalan 4 hari lalu dikunci; 71 jam masih boleh.
    let (_, sj_lama) = rek_umur(
        "surat_jalan",
        Some("TMP-11111111"),
        json!({"trip_id": "TRIP-UT-001"}),
        empat_hari,
    );
    let r = u.sinkron(vec![lot_lama, bukti_lama, sj_lama.clone()]).await;
    let s = Uji::ringkas(&r);
    assert_eq!(s[0].0, "DITERIMA", "{}", r.json);
    assert_eq!(s[1].0, "DITERIMA", "{}", r.json);
    assert_eq!(s[2].0, "DITOLAK", "{}", r.json);
    assert!(r.json["hasil"][2]["pesan"]
        .as_str()
        .unwrap()
        .contains("3x24 jam"));
    assert_eq!(u.hitung("SELECT count(*) FROM surat_jalan").await, 0);

    let (_, sj_baru) = rek_umur(
        "surat_jalan",
        Some("TMP-22222222"),
        json!({"trip_id": "TRIP-UT-001"}),
        Duration::hours(71),
    );
    let r = u.sinkron(vec![sj_baru]).await;
    assert_eq!(Uji::ringkas(&r)[0].0, "DITERIMA", "{}", r.json);
}

#[sqlx::test]
async fn payload_buruk_ditolak_per_rekaman_dan_jenis_asing_ditolak_seluruhnya(pool: PgPool) {
    let u = siapkan(pool).await;
    let (_, tmp_buruk) = rek("lot", Some("TMP-XYZ"), lot_payload(1_000));
    let (_, bukan_objek) = rek("lot", None, json!([1, 2]));
    let (_, bidang_asing) = rek(
        "lot",
        None,
        json!({"kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": "2026-10-06", "berat_kotor_g": 1_000, "fee": 1}),
    );
    let (_, uuid_beda) = rek(
        "lot",
        None,
        json!({"uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": "2026-10-06", "berat_kotor_g": 1_000}),
    );
    let (_, berat_negatif) = rek("lot", None, lot_payload(-1));
    let (_, sj_tanpa_trip) = rek("surat_jalan", None, json!({}));
    let (_, baik) = rek("lot", None, lot_payload(1_000));
    let r = u
        .sinkron(vec![
            tmp_buruk,
            bukan_objek,
            bidang_asing,
            uuid_beda,
            berat_negatif,
            sj_tanpa_trip,
            baik,
        ])
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let s = Uji::ringkas(&r);
    assert!(s[..6].iter().all(|(st, _)| st == "DITOLAK"), "{}", r.json);
    assert_eq!(s[6].0, "DITERIMA");
    for i in 0..6 {
        assert!(
            r.json["hasil"][i]["pesan"].is_string(),
            "pesan penolakan #{i}"
        );
    }
    assert_eq!(u.hitung("SELECT count(*) FROM lot").await, 1);

    // Jenis tak dikenal merusak skema permintaan: 422 untuk seluruh batch.
    let (_, asing) = rek("pesanan", None, json!({}));
    let r = u.sinkron(vec![asing]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn grade_c_di_keranjang_via_sinkron_ditolak_dan_percobaannya_tercatat(pool: PgPool) {
    let u = siapkan(pool).await;
    let (_, lot) = rek("lot", Some("TMP-99999999"), lot_payload(120_000));
    let (_, sortir) = rek(
        "sortir",
        None,
        json!({"lot_id": "TMP-99999999", "grade_a_g": 100_000, "grade_b_g": 14_000, "grade_c_g": 6_000}),
    );
    let (_, krj) = rek(
        "keranjang",
        None,
        json!({"keranjang": [{"kode": "KRJ-001", "lot_id": "TMP-99999999", "grade": "C", "berat_g": 1_000}]}),
    );
    let r = u.sinkron(vec![lot, sortir, krj]).await;
    let s = Uji::ringkas(&r);
    assert_eq!(s[2].0, "DITOLAK", "{}", r.json);
    assert!(r.json["hasil"][2]["pesan"]
        .as_str()
        .unwrap()
        .contains("Grade C"));
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_isi").await, 0);
    assert_eq!(
        u.hitung("SELECT count(*) FROM audit_log WHERE aksi = 'grade_c_ditolak_rantai_dingin'")
            .await,
        1
    );
}

#[sqlx::test]
async fn sinkron_hanya_untuk_operator_gudang(pool: PgPool) {
    let u = siapkan(pool).await;
    let (_, lot) = rek("lot", None, lot_payload(1_000));
    let badan = json!({"rekaman": [lot]});
    for (nama, header, harap) in [
        ("tanpa kredensial", vec![], StatusCode::UNAUTHORIZED),
        (
            "device key saja",
            vec![("x-device-key", KUNCI_GUDANG)],
            StatusCode::UNAUTHORIZED,
        ),
        (
            "service key",
            vec![("x-service-key", KUNCI_LAYANAN)],
            StatusCode::UNAUTHORIZED,
        ),
        (
            "bearer tanpa device key",
            vec![("authorization", u.sesi.auth.as_str())],
            StatusCode::UNAUTHORIZED,
        ),
        (
            "sopir + device key",
            vec![
                ("authorization", u.sopir.as_str()),
                ("x-device-key", KUNCI_GUDANG),
            ],
            StatusCode::FORBIDDEN,
        ),
    ] {
        let r = kirim(
            &u.app,
            "POST",
            "/v1/sinkron/batch",
            &header,
            Some(badan.clone()),
        )
        .await;
        assert_eq!(r.status, harap, "{nama}");
    }
    assert_eq!(u.hitung("SELECT count(*) FROM lot").await, 0);
}
