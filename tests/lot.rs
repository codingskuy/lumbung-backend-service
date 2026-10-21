//! Uji LMB-BE-004: lot, sortir +-2%, keranjang batch (maks 50), stok-lot, Grade C, idempotensi.

mod common;

use axum::{http::StatusCode, Router};
use common::*;
use lumbung_backend_service::auth::peran::Peran;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

struct Uji {
    pool: PgPool,
    app: Router,
    sesi: SesiGudang,
}

async fn siapkan(pool: PgPool) -> Uji {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let sesi = masuk_gudang(&app, &pool).await;
    buat_petani(&pool, "PTN-007").await;
    Uji { pool, app, sesi }
}

impl Uji {
    async fn post(&self, uri: &str, badan: Value) -> Resp {
        kirim(&self.app, "POST", uri, &self.sesi.h(), Some(badan)).await
    }

    async fn buat_lot(&self, kotor: i64) -> Resp {
        self.post(
            "/v1/lot",
            json!({
                "uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai",
                "tanggal": "2026-10-06", "berat_kotor_g": kotor, "petugas": "OPR-SORTIR-1"
            }),
        )
        .await
    }

    async fn sortir(&self, kode: &str, a: i64, b: i64, c: i64) -> Resp {
        self.post(
            &format!("/v1/lot/{kode}/sortir"),
            json!({"uuid": Uuid::now_v7(), "lot_id": kode, "grade_a_g": a, "grade_b_g": b, "grade_c_g": c}),
        )
        .await
    }

    /// Lot 120.000 g tersortir A 100.000 / B 14.000 / C 6.000 (contoh L-8812).
    async fn lot_tersortir(&self) -> String {
        let kode = self.buat_lot(120_000).await.json["kode"]
            .as_str()
            .unwrap()
            .to_string();
        let r = self.sortir(&kode, 100_000, 14_000, 6_000).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        kode
    }

    async fn batch(&self, keranjang: Value) -> Resp {
        self.post(
            "/v1/keranjang/batch",
            json!({"uuid": Uuid::now_v7(), "keranjang": keranjang}),
        )
        .await
    }

    async fn hitung(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.pool).await.unwrap()
    }
}

fn krj(kode: &str, lot: &str, grade: &str, berat: i64) -> Value {
    json!({"kode": kode, "lot_id": lot, "grade": grade, "berat_g": berat})
}

// ---------- lot + sortir ----------

#[sqlx::test]
async fn lot_dan_sortir_contoh_l8812_tercatat_lengkap(pool: PgPool) {
    let u = siapkan(pool).await;
    let r = u.buat_lot(120_000).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let kode = r.json["kode"].as_str().unwrap().to_string();
    assert!(
        kode.len() == 6 && kode.starts_with("L-"),
        "kode resmi L-NNNN: {kode}"
    );
    assert_eq!(r.json["status"], "BARU");
    assert_eq!(r.json["versi"], 1);
    assert_eq!(r.json["berat_kotor_g"], 120_000);
    assert_eq!(r.json["duplikat"], false);
    assert!(r.json.get("grade_a_g").is_none(), "belum disortir");
    assert!(r.json["dibuat_pada"].as_str().unwrap().ends_with("+07:00"));

    let r = u.sortir(&kode, 100_000, 14_000, 6_000).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["status"], "DISORTIR");
    assert_eq!(r.json["versi"], 2);
    assert_eq!(
        (
            r.json["grade_a_g"].as_i64(),
            r.json["grade_b_g"].as_i64(),
            r.json["grade_c_g"].as_i64()
        ),
        (Some(100_000), Some(14_000), Some(6_000))
    );

    // Timbangan KOTOR dan BERSIH; susut = kotor - bersih.
    assert_eq!(
        u.hitung("SELECT count(*) FROM timbangan WHERE jenis = 'KOTOR' AND berat_g = 120000")
            .await,
        1
    );
    assert_eq!(
        u.hitung("SELECT count(*) FROM timbangan WHERE jenis = 'BERSIH' AND berat_g = 120000")
            .await,
        1
    );
    // Grade C dialihkan dan dicatat (FR-003).
    assert_eq!(u.hitung("SELECT berat_g FROM alih_grade_c").await, 6_000);
    assert_eq!(
        u.hitung("SELECT count(*) FROM audit_log WHERE aksi = 'grade_c_dialihkan'")
            .await,
        1
    );
}

#[sqlx::test]
async fn kode_lot_menaik_dan_unik(pool: PgPool) {
    let u = siapkan(pool).await;
    let a = u.buat_lot(1_000).await.json["kode"]
        .as_str()
        .unwrap()
        .to_string();
    let b = u.buat_lot(1_000).await.json["kode"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!((a.as_str(), b.as_str()), ("L-0001", "L-0002"));
}

#[sqlx::test]
async fn sortir_di_luar_dua_persen_ditolak_422_dan_batas_tepat_diterima(pool: PgPool) {
    let u = siapkan(pool).await;
    // Batas atas +2.400 g dan bawah -2.400 g (2 persen dari 120.000).
    for (a, b, c, harap) in [
        (100_000, 14_000, 8_400, StatusCode::CREATED), // 122.400
        (100_000, 14_000, 8_401, StatusCode::UNPROCESSABLE_ENTITY), // 122.401
        (100_000, 14_000, 3_600, StatusCode::CREATED), // 117.600
        (100_000, 14_000, 3_599, StatusCode::UNPROCESSABLE_ENTITY), // 117.599
        (0, 0, 0, StatusCode::UNPROCESSABLE_ENTITY),
        (-1, 100_000, 20_001, StatusCode::UNPROCESSABLE_ENTITY), // negatif walau total pas
    ] {
        let kode = u.buat_lot(120_000).await.json["kode"]
            .as_str()
            .unwrap()
            .to_string();
        let r = u.sortir(&kode, a, b, c).await;
        assert_eq!(r.status, harap, "A{a} B{b} C{c}: {}", r.json);
        if harap == StatusCode::UNPROCESSABLE_ENTITY {
            // Lot tetap BARU: tidak ada setengah tersimpan.
            let status: String = sqlx::query_scalar("SELECT status FROM lot WHERE kode = $1")
                .bind(&kode)
                .fetch_one(&u.pool)
                .await
                .unwrap();
            assert_eq!(status, "BARU");
        }
    }
    assert_eq!(u.hitung("SELECT count(*) FROM sortir").await, 2);
}

#[sqlx::test]
async fn validasi_lot_berat_petani_dan_bidang_asing(pool: PgPool) {
    let u = siapkan(pool.clone()).await;
    sqlx::query("INSERT INTO petani (kode, nama, status) VALUES ('PTN-009', 'X', 'NONAKTIF')")
        .execute(&pool)
        .await
        .unwrap();
    let dasar = |ubah: &dyn Fn(&mut Value)| {
        let mut b = json!({"uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai",
                           "tanggal": "2026-10-06", "berat_kotor_g": 1000});
        ubah(&mut b);
        b
    };
    let kasus: Vec<(&str, Value)> = vec![
        ("berat 0", dasar(&|b| b["berat_kotor_g"] = json!(0))),
        ("berat negatif", dasar(&|b| b["berat_kotor_g"] = json!(-5))),
        (
            "berat desimal",
            dasar(&|b| b["berat_kotor_g"] = json!(10.5)),
        ),
        (
            "komoditas tak dikenal",
            dasar(&|b| b["komoditas"] = json!("tomat")),
        ),
        (
            "petani format salah",
            dasar(&|b| b["kode_petani"] = json!("PTN-7")),
        ),
        (
            "petani tak terdaftar",
            dasar(&|b| b["kode_petani"] = json!("PTN-999")),
        ),
        (
            "petani nonaktif",
            dasar(&|b| b["kode_petani"] = json!("PTN-009")),
        ),
        ("tanggal rusak", dasar(&|b| b["tanggal"] = json!("kemarin"))),
        ("bidang asing", dasar(&|b| b["fee"] = json!(1))),
    ];
    for (nama, badan) in kasus {
        let r = u.post("/v1/lot", badan).await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{nama}: {}",
            r.json
        );
    }
    assert_eq!(u.hitung("SELECT count(*) FROM lot").await, 0);
}

#[sqlx::test]
async fn semua_post_idempoten_lewat_uuid_klien(pool: PgPool) {
    let u = siapkan(pool).await;
    let uuid_lot = Uuid::now_v7();
    let badan_lot = json!({"uuid": uuid_lot, "kode_petani": "PTN-007", "komoditas": "cabai",
                           "tanggal": "2026-10-06", "berat_kotor_g": 120000});
    let pertama = u.post("/v1/lot", badan_lot.clone()).await;
    assert_eq!(pertama.status, StatusCode::CREATED);
    let ulang = u.post("/v1/lot", badan_lot).await;
    assert_eq!(ulang.status, StatusCode::OK);
    assert_eq!(ulang.json["duplikat"], true);
    assert_eq!(ulang.json["id"], pertama.json["kode"]);
    assert_eq!(u.hitung("SELECT count(*) FROM lot").await, 1);
    assert_eq!(u.hitung("SELECT count(*) FROM timbangan").await, 1);

    let kode = pertama.json["kode"].as_str().unwrap();
    let uuid_sortir = Uuid::now_v7();
    let sortir = json!({"uuid": uuid_sortir, "lot_id": kode, "grade_a_g": 100000, "grade_b_g": 14000, "grade_c_g": 6000});
    let uri = format!("/v1/lot/{kode}/sortir");
    assert_eq!(
        u.post(&uri, sortir.clone()).await.status,
        StatusCode::CREATED
    );
    let ulang = u.post(&uri, sortir).await;
    assert_eq!(ulang.status, StatusCode::OK);
    assert_eq!(ulang.json, json!({"id": kode, "duplikat": true}));
    assert_eq!(u.hitung("SELECT count(*) FROM sortir").await, 1);
    assert_eq!(u.hitung("SELECT count(*) FROM alih_grade_c").await, 1);

    let batch = json!({"uuid": Uuid::now_v7(), "keranjang": [krj("KRJ-001", kode, "A", 20_000)]});
    assert_eq!(
        u.post("/v1/keranjang/batch", batch.clone()).await.status,
        StatusCode::CREATED
    );
    let ulang = u.post("/v1/keranjang/batch", batch).await;
    assert_eq!(ulang.status, StatusCode::OK);
    assert_eq!(ulang.json["duplikat"], true);
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_isi").await, 1);
}

#[sqlx::test]
async fn sortir_kedua_404_409_dan_ketidakcocokan_id_422(pool: PgPool) {
    let u = siapkan(pool).await;
    let kode = u.lot_tersortir().await;
    // Sortir ulang dengan UUID baru: konflik versi.
    let r = u.sortir(&kode, 100_000, 14_000, 6_000).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json["error"], "konflik versi, data gudang lebih baru");
    // Lot tak ada.
    assert_eq!(
        u.sortir("L-9999", 1, 1, 1).await.status,
        StatusCode::NOT_FOUND
    );
    // lot_id badan berbeda dari path.
    let lain = u.buat_lot(120_000).await.json["kode"]
        .as_str()
        .unwrap()
        .to_string();
    let r = u
        .post(
            &format!("/v1/lot/{lain}/sortir"),
            json!({"uuid": Uuid::now_v7(), "lot_id": kode, "grade_a_g": 120000, "grade_b_g": 0, "grade_c_g": 0}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn lot_dengan_nomor_sementara_dikenali_di_path_sortir(pool: PgPool) {
    let u = siapkan(pool).await;
    let kode = u.buat_lot(120_000).await.json["kode"]
        .as_str()
        .unwrap()
        .to_string();
    sqlx::query("UPDATE lot SET nomor_sementara = 'TMP-0a1b2c3d' WHERE kode = $1")
        .bind(&kode)
        .execute(&u.pool)
        .await
        .unwrap();
    let r = u
        .post(
            "/v1/lot/TMP-0a1b2c3d/sortir",
            json!({"uuid": Uuid::now_v7(), "lot_id": "TMP-0a1b2c3d", "grade_a_g": 100000, "grade_b_g": 14000, "grade_c_g": 6000}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["kode"], kode.as_str());
}

// ---------- keranjang + stok ----------

#[sqlx::test]
async fn keranjang_25_kali_20_kg_dan_stok_lot_menyusut(pool: PgPool) {
    let u = siapkan(pool).await;
    let kode = u.lot_tersortir().await;

    let stok = kirim(&u.app, "GET", "/v1/stok-lot", &u.sesi.h(), None).await;
    assert_eq!(stok.status, StatusCode::OK);
    assert_eq!(
        stok.json["data"],
        json!([
            {"lot_id": kode, "komoditas": "cabai", "grade": "A", "sisa_g": 100_000},
            {"lot_id": kode, "komoditas": "cabai", "grade": "B", "sisa_g": 14_000},
        ]),
        "Grade C tidak masuk stok rantai dingin"
    );

    // 25 keranjang x 20 kg = 500 kg diuji pada lot lebih besar; di sini 5 x 20.000 g = 100.000 g Grade A.
    let isi: Vec<Value> = (1..=5)
        .map(|i| krj(&format!("KRJ-{i:03}"), &kode, "A", 20_000))
        .collect();
    let r = u.batch(json!(isi)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["duplikat"], false);

    let stok = kirim(&u.app, "GET", "/v1/stok-lot", &u.sesi.h(), None).await;
    assert_eq!(
        stok.json["data"],
        json!([{"lot_id": kode, "komoditas": "cabai", "grade": "B", "sisa_g": 14_000}]),
        "Grade A habis dan tidak muncul lagi"
    );
}

#[sqlx::test]
async fn batch_maksimal_50_dan_25_keranjang_500_kg_lolos(pool: PgPool) {
    let u = siapkan(pool).await;
    // Lot besar: 1.000 kg, Grade A 1.000.000 g.
    let kode = u.buat_lot(1_000_000).await.json["kode"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        u.sortir(&kode, 1_000_000, 0, 0).await.status,
        StatusCode::CREATED
    );

    let dua_puluh_lima: Vec<Value> = (1..=25)
        .map(|i| krj(&format!("KRJ-{i:03}"), &kode, "A", 20_000))
        .collect();
    assert_eq!(
        u.batch(json!(dua_puluh_lima)).await.status,
        StatusCode::CREATED
    );
    let jumlah: i64 = u
        .hitung("SELECT SUM(berat_g)::bigint FROM keranjang_isi")
        .await;
    assert_eq!(jumlah, 500_000, "25 x 20 kg = 500 kg");

    // Tepat 50 lolos, 51 ditolak 422, kosong ditolak 422.
    let lima_puluh: Vec<Value> = (26..=75)
        .map(|i| krj(&format!("KRJ-{i:03}"), &kode, "A", 1_000))
        .collect();
    assert_eq!(u.batch(json!(lima_puluh)).await.status, StatusCode::CREATED);
    let terlalu_banyak: Vec<Value> = (0..51)
        .map(|i| krj(&format!("KRJ-{:03}", (i % 50) + 1), &kode, "A", 1))
        .collect();
    let r = u.batch(json!(terlalu_banyak)).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        r.json["error"].as_str().unwrap().contains("maksimal 50"),
        "{}",
        r.json
    );
    assert_eq!(
        u.batch(json!([])).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_isi").await, 75);
}

#[sqlx::test]
async fn batch_atomik_gagal_sebagian_tidak_menyimpan_apa_pun(pool: PgPool) {
    let u = siapkan(pool).await;
    let kode = u.lot_tersortir().await;
    // Item ke-2 melebihi stok Grade B (14.000 g): seluruh batch gagal.
    let r = u
        .batch(json!([
            krj("KRJ-001", &kode, "A", 10_000),
            krj("KRJ-002", &kode, "B", 14_001)
        ]))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.json);
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_isi").await, 0);
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_batch").await, 0);
    // Stok tepat habis diterima.
    let r = u
        .batch(json!([
            krj("KRJ-001", &kode, "A", 100_000),
            krj("KRJ-002", &kode, "B", 14_000)
        ]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
}

#[sqlx::test]
async fn grade_c_ditolak_dari_rantai_dingin_dan_dicatat(pool: PgPool) {
    let u = siapkan(pool).await;
    let kode = u.lot_tersortir().await;
    let r = u.batch(json!([krj("KRJ-001", &kode, "C", 1_000)])).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        r.json["error"].as_str().unwrap().contains("Grade C"),
        "{}",
        r.json
    );
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_isi").await, 0);
    assert_eq!(
        u.hitung("SELECT count(*) FROM audit_log WHERE aksi = 'grade_c_ditolak_rantai_dingin'")
            .await,
        1
    );
    // Grade tak dikenal ditolak di tingkat skema.
    let r = u.batch(json!([krj("KRJ-001", &kode, "D", 1_000)])).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test]
async fn validasi_keranjang_kode_berat_lot_dan_status(pool: PgPool) {
    let u = siapkan(pool).await;
    let tersortir = u.lot_tersortir().await;
    let baru = u.buat_lot(120_000).await.json["kode"]
        .as_str()
        .unwrap()
        .to_string();

    for (nama, item, harap) in [
        (
            "KRJ-000",
            krj("KRJ-000", &tersortir, "A", 1),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "KRJ-121",
            krj("KRJ-121", &tersortir, "A", 1),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "kode salah",
            krj("BSK-001", &tersortir, "A", 1),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "berat 0",
            krj("KRJ-001", &tersortir, "A", 0),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "berat negatif",
            krj("KRJ-001", &tersortir, "A", -1),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "lot tak ada",
            krj("KRJ-001", "L-9999", "A", 1),
            StatusCode::NOT_FOUND,
        ),
        (
            "lot belum disortir",
            krj("KRJ-001", &baru, "A", 1),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let r = u.batch(json!([item])).await;
        assert_eq!(r.status, harap, "{nama}: {}", r.json);
    }
    // Kode dobel dalam satu batch.
    let r = u
        .batch(json!([
            krj("KRJ-001", &tersortir, "A", 1),
            krj("KRJ-001", &tersortir, "A", 1)
        ]))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(u.hitung("SELECT count(*) FROM keranjang_isi").await, 0);
}

#[sqlx::test]
async fn keranjang_yang_masih_terisi_tidak_bisa_diisi_ulang(pool: PgPool) {
    let u = siapkan(pool).await;
    let kode = u.lot_tersortir().await;
    assert_eq!(
        u.batch(json!([krj("KRJ-001", &kode, "A", 1_000)]))
            .await
            .status,
        StatusCode::CREATED
    );
    let r = u.batch(json!([krj("KRJ-001", &kode, "A", 1_000)])).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.json);
    // Setelah dikosongkan (selesai), kode fisik boleh dipakai lagi.
    sqlx::query("UPDATE keranjang_isi SET selesai_at = now()")
        .execute(&u.pool)
        .await
        .unwrap();
    assert_eq!(
        u.batch(json!([krj("KRJ-001", &kode, "A", 1_000)]))
            .await
            .status,
        StatusCode::CREATED
    );
}

#[sqlx::test]
async fn dua_batch_bersamaan_tidak_melewati_stok(pool: PgPool) {
    let u = siapkan(pool).await;
    let kode = u.lot_tersortir().await; // Grade A 100.000 g
    let batch = |nomor: &str| {
        let badan = json!({"uuid": Uuid::now_v7(), "keranjang": [krj(nomor, &kode, "A", 60_000)]});
        let app = u.app.clone();
        let h = u.sesi.auth.clone();
        async move {
            kirim(
                &app,
                "POST",
                "/v1/keranjang/batch",
                &[("authorization", &h), ("x-device-key", KUNCI_GUDANG)],
                Some(badan),
            )
            .await
        }
    };
    let (a, b) = tokio::join!(batch("KRJ-001"), batch("KRJ-002"));
    let mut hasil = [a.status, b.status];
    hasil.sort();
    assert_eq!(
        hasil,
        [StatusCode::CREATED, StatusCode::UNPROCESSABLE_ENTITY],
        "tepat satu lolos"
    );
    let total: i64 = u
        .hitung("SELECT SUM(berat_g)::bigint FROM keranjang_isi")
        .await;
    assert_eq!(total, 60_000);
}

// ---------- otorisasi ----------

#[sqlx::test]
async fn endpoint_gudang_wajib_bearer_dan_device_key_serta_peran_admin_gudang(pool: PgPool) {
    let u = siapkan(pool.clone()).await;
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let sopir = bearer(&masuk(&u.app, "sopir-02").await.0);
    let badan_lot = json!({"uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai",
                           "tanggal": "2026-10-06", "berat_kotor_g": 1000});
    let kode = u.buat_lot(1_000).await.json["kode"]
        .as_str()
        .unwrap()
        .to_string();
    let rute: Vec<(&str, String, Option<Value>)> = vec![
        ("POST", "/v1/lot".into(), Some(badan_lot)),
        (
            "POST",
            format!("/v1/lot/{kode}/sortir"),
            Some(
                json!({"uuid": Uuid::now_v7(), "lot_id": kode, "grade_a_g": 1000, "grade_b_g": 0, "grade_c_g": 0}),
            ),
        ),
        (
            "POST",
            "/v1/keranjang/batch".into(),
            Some(json!({"uuid": Uuid::now_v7(), "keranjang": []})),
        ),
        ("GET", "/v1/stok-lot".into(), None),
    ];
    for (metode, uri, badan) in rute {
        // Tanpa kredensial, device key saja, service key saja: 401.
        for header in [
            vec![],
            vec![("x-device-key", KUNCI_GUDANG)],
            vec![("x-service-key", KUNCI_LAYANAN)],
        ] {
            let r = kirim(&u.app, metode, &uri, &header, badan.clone()).await;
            assert_eq!(
                r.status,
                StatusCode::UNAUTHORIZED,
                "{metode} {uri} {header:?}"
            );
        }
        // Bearer tanpa device key: 401. Sopir dengan device key: 403.
        let r = kirim(
            &u.app,
            metode,
            &uri,
            &[("authorization", &u.sesi.auth)],
            badan.clone(),
        )
        .await;
        assert_eq!(
            r.status,
            StatusCode::UNAUTHORIZED,
            "{metode} {uri} tanpa device key"
        );
        let r = kirim(
            &u.app,
            metode,
            &uri,
            &[("authorization", &sopir), ("x-device-key", KUNCI_GUDANG)],
            badan,
        )
        .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{metode} {uri} sopir");
    }
    // Penolakan peran dicatat.
    assert_eq!(
        u.hitung(
            "SELECT count(*) FROM audit_log WHERE aksi = 'akses_ditolak' AND aktor = 'sopir-02'"
        )
        .await,
        4
    );
}
