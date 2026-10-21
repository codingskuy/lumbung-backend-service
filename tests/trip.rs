//! Uji LMB-BE-005: trip, surat jalan SJ-*, endpoint sopir, foto, insiden.

mod common;

use axum::{http::StatusCode, Router};
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

fn today() -> String {
    waktu::hari_ini_wib().to_string()
}

fn today_compact() -> String {
    waktu::hari_ini_wib().format("%Y%m%d").to_string()
}

impl Uji {
    async fn gudang(&self, uri: &str, badan: Value) -> Resp {
        kirim(&self.app, "POST", uri, &self.sesi.h(), Some(badan)).await
    }

    async fn sopir_post(&self, uri: &str, badan: Value) -> Resp {
        kirim(
            &self.app,
            "POST",
            uri,
            &[("authorization", &self.sopir)],
            Some(badan),
        )
        .await
    }

    async fn hitung(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.pool).await.unwrap()
    }

    /// Lot 1.000 kg tersortir (Grade A 1.000.000 g) dengan keranjang yang diminta (masing-masing 10.000 g).
    async fn lot_dengan_keranjang(&self, kodes: &[&str]) -> String {
        let r = self
            .gudang("/v1/lot", json!({"uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": "2026-10-06", "berat_kotor_g": 1_000_000}))
            .await;
        let kode = r.json["kode"].as_str().unwrap().to_string();
        let r = self
            .gudang(&format!("/v1/lot/{kode}/sortir"), json!({"uuid": Uuid::now_v7(), "lot_id": kode, "grade_a_g": 1_000_000, "grade_b_g": 0, "grade_c_g": 0}))
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        let isi: Vec<Value> = kodes
            .iter()
            .map(|k| json!({"kode": k, "lot_id": kode, "grade": "A", "berat_g": 10_000}))
            .collect();
        let r = self
            .gudang(
                "/v1/keranjang/batch",
                json!({"uuid": Uuid::now_v7(), "keranjang": isi}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        kode
    }

    fn badan_trip(&self, keranjang: &[&str]) -> Value {
        json!({
            "uuid": Uuid::now_v7(), "rute": "UTARA", "kendaraan": "L 1234 AB",
            "keranjang": keranjang, "sopir": "sopir-02",
            "titik": [
                {"id": "PWN-1", "zona": "DEKAT", "pesanan": "PSO-20261010-001", "keranjang": [keranjang[0]]},
                {"id": "LPK-2", "zona": "JAUH", "keranjang": keranjang[1..]},
            ],
        })
    }

    /// Trip siap jalan dengan 2 keranjang; mengembalikan kode trip.
    async fn trip_siap(&self) -> String {
        self.lot_dengan_keranjang(&["KRJ-001", "KRJ-002"]).await;
        let r = self
            .gudang("/v1/trip", self.badan_trip(&["KRJ-001", "KRJ-002"]))
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        r.json["kode"].as_str().unwrap().to_string()
    }

    fn titik_badan(&self, trip: &str, titik: &str, waktu: &str) -> Value {
        json!({"uuid": Uuid::now_v7(), "trip_id": trip, "titik_id": titik, "waktu": waktu, "lat": -7.79, "lng": 110.36})
    }
}

// ---------- trip ----------

#[sqlx::test]
async fn trip_dibuat_dengan_kode_kas_jalan_titik_dan_lot_dimuat(pool: PgPool) {
    let u = siapkan(pool).await;
    let lot = u.lot_dengan_keranjang(&["KRJ-001", "KRJ-002"]).await;
    let r = u
        .gudang("/v1/trip", u.badan_trip(&["KRJ-001", "KRJ-002"]))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["kode"], "TRIP-UT-001");
    assert_eq!(r.json["rute"], "UTARA");
    assert_eq!(r.json["kas_jalan"], 300_000);
    assert_eq!(r.json["status"], "DISUSUN");
    assert_eq!(r.json["sopir"], "sopir-02");
    assert_eq!(
        r.json["titik"][0],
        json!({"id": "PWN-1", "urutan": 1, "zona": "DEKAT", "tiba_pada": null, "berangkat_pada": null})
    );
    assert_eq!(r.json["titik"][1]["id"], "LPK-2");
    assert_eq!(r.json["titik"][1]["zona"], "JAUH");

    let status: String = sqlx::query_scalar("SELECT status FROM lot WHERE kode = $1")
        .bind(&lot)
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(status, "DIMUAT");
    assert_eq!(
        u.hitung("SELECT count(*) FROM keranjang_isi WHERE trip_id IS NOT NULL")
            .await,
        2
    );
    assert_eq!(
        u.hitung(
            "SELECT count(*) FROM keranjang_isi WHERE titik_id = 'PWN-1' AND kode = 'KRJ-001'"
        )
        .await,
        1
    );
}

#[sqlx::test]
async fn nomor_trip_per_rute_menaik_dan_idempoten(pool: PgPool) {
    let u = siapkan(pool).await;
    u.lot_dengan_keranjang(&["KRJ-001", "KRJ-002", "KRJ-003"])
        .await;
    let a = u
        .gudang("/v1/trip", u.badan_trip(&["KRJ-001", "KRJ-002"]))
        .await;
    assert_eq!(a.json["kode"], "TRIP-UT-001");
    let mut selatan = json!({"uuid": Uuid::now_v7(), "rute": "SELATAN", "kendaraan": "B 1", "keranjang": ["KRJ-003"]});
    let b = u.gudang("/v1/trip", selatan.clone()).await;
    assert_eq!(b.status, StatusCode::CREATED, "{}", b.json);
    assert_eq!(
        b.json["kode"], "TRIP-SL-001",
        "penghitung terpisah per rute"
    );
    // Kirim ulang UUID yang sama: 200 duplikat, tanpa trip ganda.
    let ulang = u.gudang("/v1/trip", selatan.take()).await;
    assert_eq!(ulang.status, StatusCode::OK);
    assert_eq!(ulang.json, json!({"id": "TRIP-SL-001", "duplikat": true}));
    assert_eq!(u.hitung("SELECT count(*) FROM trip").await, 2);
}

#[sqlx::test]
async fn validasi_trip(pool: PgPool) {
    let u = siapkan(pool).await;
    u.lot_dengan_keranjang(&["KRJ-001", "KRJ-002"]).await;
    let dasar = || json!({"uuid": Uuid::now_v7(), "rute": "KOTA", "kendaraan": "L 1", "keranjang": ["KRJ-001"]});
    let ubah = |f: &dyn Fn(&mut Value)| {
        let mut b = dasar();
        f(&mut b);
        b
    };
    for (nama, badan, harap) in [
        (
            "tanpa keranjang",
            ubah(&|b| b["keranjang"] = json!([])),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "keranjang dobel",
            ubah(&|b| b["keranjang"] = json!(["KRJ-001", "KRJ-001"])),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "kode keranjang salah",
            ubah(&|b| b["keranjang"] = json!(["KRJ-999"])),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "keranjang tak terisi",
            ubah(&|b| b["keranjang"] = json!(["KRJ-050"])),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "kas jalan salah",
            ubah(&|b| b["kas_jalan"] = json!(250_000)),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "rute tak dikenal",
            ubah(&|b| b["rute"] = json!("BARAT")),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "kendaraan kosong",
            ubah(&|b| b["kendaraan"] = json!("  ")),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "sopir bukan sopir",
            ubah(&|b| b["sopir"] = json!("OPR-SORTIR-1")),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "sopir tak ada",
            ubah(&|b| b["sopir"] = json!("sopir-99")),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "titik dobel",
            ubah(&|b| {
                b["titik"] = json!([{"id": "A", "zona": "DEKAT"}, {"id": "A", "zona": "JAUH"}])
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "zona salah",
            ubah(&|b| b["titik"] = json!([{"id": "A", "zona": "TENGAH"}])),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "pesanan salah",
            ubah(&|b| b["titik"] = json!([{"id": "A", "zona": "DEKAT", "pesanan": "PSO-1"}])),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "keranjang titik asing",
            ubah(&|b| b["titik"] = json!([{"id": "A", "zona": "DEKAT", "keranjang": ["KRJ-002"]}])),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "bidang asing",
            ubah(&|b| b["fee"] = json!(1)),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        ("sah", dasar(), StatusCode::CREATED),
    ] {
        let r = u.gudang("/v1/trip", badan).await;
        assert_eq!(r.status, harap, "{nama}: {}", r.json);
    }
    assert_eq!(
        u.hitung("SELECT count(*) FROM trip").await,
        1,
        "hanya kasus sah yang tersimpan"
    );
    // Keranjang yang sudah dimuat tidak bisa dimuat lagi ke trip lain.
    let r = u.gudang("/v1/trip", json!({"uuid": Uuid::now_v7(), "rute": "KOTA", "kendaraan": "L 2", "keranjang": ["KRJ-001"]})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        r.json["error"].as_str().unwrap().contains("sudah dimuat"),
        "{}",
        r.json
    );
}

// ---------- surat jalan ----------

#[sqlx::test]
async fn surat_jalan_nomor_sj_tanggal_urut_idempoten_dan_satu_per_trip(pool: PgPool) {
    let u = siapkan(pool).await;
    let trip = u.trip_siap().await;
    let uuid = Uuid::now_v7();
    let r = u
        .gudang(
            &format!("/v1/trip/{trip}/surat-jalan"),
            json!({"uuid": uuid}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["nomor"], format!("SJ-{}-001", today_compact()));
    assert_eq!(r.json["trip"], trip.as_str());
    assert_eq!(r.json["nomor_sementara"], Value::Null);
    assert_eq!(
        r.json["keranjang"],
        json!([{"kode": "KRJ-001", "grade": "A", "berat_g": 10_000}, {"kode": "KRJ-002", "grade": "A", "berat_g": 10_000}])
    );

    // Kirim ulang UUID sama: 200 duplikat, nomor tidak bertambah.
    let r2 = u
        .gudang(
            &format!("/v1/trip/{trip}/surat-jalan"),
            json!({"uuid": uuid}),
        )
        .await;
    assert_eq!(r2.status, StatusCode::OK);
    assert_eq!(
        r2.json,
        json!({"id": format!("SJ-{}-001", today_compact()), "duplikat": true})
    );
    // UUID lain untuk trip yang sama: 409. Trip tak ada: 404.
    let r3 = u
        .gudang(
            &format!("/v1/trip/{trip}/surat-jalan"),
            json!({"uuid": Uuid::now_v7()}),
        )
        .await;
    assert_eq!(r3.status, StatusCode::CONFLICT);
    let r4 = u
        .gudang(
            "/v1/trip/TRIP-UT-999/surat-jalan",
            json!({"uuid": Uuid::now_v7()}),
        )
        .await;
    assert_eq!(r4.status, StatusCode::NOT_FOUND);
    assert_eq!(u.hitung("SELECT count(*) FROM surat_jalan").await, 1);

    // Trip lain mendapat nomor berikutnya hari itu.
    u.lot_dengan_keranjang(&["KRJ-003"]).await;
    let t2 = u.gudang("/v1/trip", json!({"uuid": Uuid::now_v7(), "rute": "KOTA", "kendaraan": "L 2", "keranjang": ["KRJ-003"]})).await;
    let kode2 = t2.json["kode"].as_str().unwrap();
    let r5 = u
        .gudang(
            &format!("/v1/trip/{kode2}/surat-jalan"),
            json!({"uuid": Uuid::now_v7()}),
        )
        .await;
    assert_eq!(r5.json["nomor"], format!("SJ-{}-002", today_compact()));
}

#[sqlx::test]
async fn surat_jalan_balapan_dua_permintaan_menghasilkan_satu(pool: PgPool) {
    let u = siapkan(pool).await;
    let trip = u.trip_siap().await;
    let kirim_sj = || {
        let app = u.app.clone();
        let auth = u.sesi.auth.clone();
        let uri = format!("/v1/trip/{trip}/surat-jalan");
        async move {
            kirim(
                &app,
                "POST",
                &uri,
                &[("authorization", &auth), ("x-device-key", KUNCI_GUDANG)],
                Some(json!({"uuid": Uuid::now_v7()})),
            )
            .await
        }
    };
    let (a, b) = tokio::join!(kirim_sj(), kirim_sj());
    let mut hasil = [a.status, b.status];
    hasil.sort();
    assert_eq!(hasil, [StatusCode::CREATED, StatusCode::CONFLICT]);
    assert_eq!(u.hitung("SELECT count(*) FROM surat_jalan").await, 1);
}

// ---------- sopir ----------

#[sqlx::test]
async fn login_sopir_memuat_trip_hari_ini_dan_endpoint_hanya_milik_sopir(pool: PgPool) {
    let u = siapkan(pool.clone()).await;
    let trip = u.trip_siap().await;
    buat_pengguna(&pool, "sopir-03", Peran::Sopir).await;

    let r = kirim(
        &u.app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "sopir-02", "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.json["trip_hari_ini"], json!([trip]));
    let r = kirim(
        &u.app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "sopir-03", "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.json["trip_hari_ini"], json!([]));
    // Peran lain tidak mendapat bidang trip_hari_ini.
    let r = kirim(
        &u.app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "OPR-SORTIR-1", "pin": PIN_UJI})),
    )
    .await;
    assert!(r.json.get("trip_hari_ini").is_none());

    let r = kirim(
        &u.app,
        "GET",
        "/v1/trip-hari-ini",
        &[("authorization", &u.sopir)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["data"].as_array().unwrap().len(), 1);
    assert_eq!(r.json["data"][0]["kode"], trip.as_str());
    assert_eq!(r.json["data"][0]["titik"][0]["id"], "PWN-1");

    // Sopir lain tidak melihat trip ini; trip tanggal lain tidak tampil.
    let lain = bearer(&masuk(&u.app, "sopir-03").await.0);
    let r = kirim(
        &u.app,
        "GET",
        "/v1/trip-hari-ini",
        &[("authorization", &lain)],
        None,
    )
    .await;
    assert_eq!(r.json["data"], json!([]));
    sqlx::query("UPDATE trip SET tanggal = tanggal - 1")
        .execute(&u.pool)
        .await
        .unwrap();
    let r = kirim(
        &u.app,
        "GET",
        "/v1/trip-hari-ini",
        &[("authorization", &u.sopir)],
        None,
    )
    .await;
    assert_eq!(r.json["data"], json!([]), "hanya trip hari ini");
}

#[sqlx::test]
async fn tiba_berangkat_menggerakkan_status_trip_dan_menutup_lot(pool: PgPool) {
    let u = siapkan(pool).await;
    let trip = u.trip_siap().await;
    let h = [("authorization", u.sopir.as_str())];
    let t1 = "2026-10-10T07:15:00+07:00";

    // Berangkat sebelum tiba ditolak.
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/berangkat",
        &h,
        Some(u.titik_badan(&trip, "PWN-1", t1)),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.json);

    let tiba = u.titik_badan(&trip, "PWN-1", t1);
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/tiba",
        &h,
        Some(tiba.clone()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["duplikat"], false);
    assert_eq!(r.json["id"], tiba["uuid"]);
    // Kirim ulang UUID sama: 200 duplikat.
    let r = kirim(&u.app, "POST", "/v1/titik/PWN-1/tiba", &h, Some(tiba)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["duplikat"], true);
    let status: String = sqlx::query_scalar("SELECT status FROM trip")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(status, "BERJALAN");

    // Berangkat lebih awal dari tiba ditolak; yang benar diterima.
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/berangkat",
        &h,
        Some(u.titik_badan(&trip, "PWN-1", "2026-10-10T07:00:00+07:00")),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/berangkat",
        &h,
        Some(u.titik_badan(&trip, "PWN-1", "2026-10-10T07:30:00+07:00")),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let status: String = sqlx::query_scalar("SELECT status FROM trip")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(status, "BERJALAN", "titik kedua belum berangkat");

    // Titik kedua: setelah berangkat trip SELESAI, keranjang dikosongkan, lot DITERIMA.
    kirim(
        &u.app,
        "POST",
        "/v1/titik/LPK-2/tiba",
        &h,
        Some(u.titik_badan(&trip, "LPK-2", "2026-10-10T08:00:00+07:00")),
    )
    .await;
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/LPK-2/berangkat",
        &h,
        Some(u.titik_badan(&trip, "LPK-2", "2026-10-10T08:20:00+07:00")),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let status: String = sqlx::query_scalar("SELECT status FROM trip")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(status, "SELESAI");
    assert_eq!(
        u.hitung("SELECT count(*) FROM keranjang_isi WHERE selesai_at IS NULL")
            .await,
        0
    );
    let lot: String = sqlx::query_scalar("SELECT status FROM lot")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(lot, "DITERIMA");
    assert_eq!(u.hitung("SELECT count(*) FROM titik_event").await, 4);

    // Waktu tampil dengan offset WIB di trip-hari-ini.
    let r = kirim(&u.app, "GET", "/v1/trip-hari-ini", &h, None).await;
    assert_eq!(
        r.json["data"][0]["titik"][0]["tiba_pada"],
        "2026-10-10T07:15:00+07:00"
    );
}

#[sqlx::test]
async fn validasi_dan_otorisasi_titik(pool: PgPool) {
    let u = siapkan(pool.clone()).await;
    let trip = u.trip_siap().await;
    buat_pengguna(&pool, "sopir-03", Peran::Sopir).await;
    let lain = bearer(&masuk(&u.app, "sopir-03").await.0);
    let t = "2026-10-10T07:15:00+07:00";
    let h = [("authorization", u.sopir.as_str())];

    // Path berbeda dari badan, koordinat di luar rentang, waktu rusak, titik/trip tak ada.
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/LPK-2/tiba",
        &h,
        Some(u.titik_badan(&trip, "PWN-1", t)),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let mut b = u.titik_badan(&trip, "PWN-1", t);
    b["lat"] = json!(91.0);
    assert_eq!(
        kirim(&u.app, "POST", "/v1/titik/PWN-1/tiba", &h, Some(b))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut b = u.titik_badan(&trip, "PWN-1", t);
    b["lng"] = json!(-181.0);
    assert_eq!(
        kirim(&u.app, "POST", "/v1/titik/PWN-1/tiba", &h, Some(b))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut b = u.titik_badan(&trip, "PWN-1", t);
    b["waktu"] = json!("kemarin");
    assert_eq!(
        kirim(&u.app, "POST", "/v1/titik/PWN-1/tiba", &h, Some(b))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/ZZZ-9/tiba",
        &h,
        Some(u.titik_badan(&trip, "ZZZ-9", t)),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/tiba",
        &h,
        Some(u.titik_badan("TRIP-UT-999", "PWN-1", t)),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Sopir lain: 403 dan tercatat. Tanpa token: 401. Gudang (bukan sopir): 403.
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/tiba",
        &[("authorization", &lain)],
        Some(u.titik_badan(&trip, "PWN-1", t)),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(
        u.hitung(
            "SELECT count(*) FROM audit_log WHERE aksi = 'akses_ditolak' AND aktor = 'sopir-03'"
        )
        .await,
        1
    );
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/tiba",
        &[],
        Some(u.titik_badan(&trip, "PWN-1", t)),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = kirim(
        &u.app,
        "POST",
        "/v1/titik/PWN-1/tiba",
        &u.sesi.h(),
        Some(u.titik_badan(&trip, "PWN-1", t)),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(
        u.hitung("SELECT count(*) FROM titik_event").await,
        0,
        "semua penolakan tidak menyimpan apa pun"
    );
}

#[sqlx::test]
async fn sopir_tidak_boleh_memakai_endpoint_gudang_dan_sebaliknya(pool: PgPool) {
    let u = siapkan(pool).await;
    let gudang_dengan_sopir = [
        ("authorization", u.sopir.as_str()),
        ("x-device-key", KUNCI_GUDANG),
    ];
    for uri in [
        "/v1/trip",
        "/v1/sinkron/batch",
        "/v1/lot",
        "/v1/keranjang/batch",
    ] {
        let r = kirim(&u.app, "POST", uri, &gudang_dengan_sopir, Some(json!({}))).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{uri}");
    }
    let r = kirim(&u.app, "GET", "/v1/trip-hari-ini", &u.sesi.h(), None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    for uri in [
        "/v1/trip",
        "/v1/sinkron/batch",
        "/v1/trip-hari-ini",
        "/v1/insiden",
        "/v1/bukti-terima",
    ] {
        let metode = if uri == "/v1/trip-hari-ini" {
            "GET"
        } else {
            "POST"
        };
        let r = kirim(&u.app, metode, uri, &[], Some(json!({}))).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
}

// ---------- foto + bukti terima ----------

async fn unggah(u: &Uji, uuid: Option<&str>, berkas: &[u8]) -> Resp {
    let (ct, badan) = multipart_foto(uuid, berkas);
    kirim_bytes(
        &u.app,
        "/v1/unggah-foto",
        &[("authorization", &u.sopir)],
        &ct,
        badan,
    )
    .await
}

#[sqlx::test]
async fn unggah_foto_jpeg_maks_500kb_idempoten_dan_validasi(pool: PgPool) {
    let u = siapkan(pool).await;
    let id = Uuid::now_v7().to_string();
    let r = unggah(&u, Some(&id), &jpeg_uji(10_000)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json, json!({"foto_id": id, "duplikat": false}));
    let r = unggah(&u, Some(&id), &jpeg_uji(10_000)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json, json!({"id": id, "duplikat": true}));
    assert_eq!(u.hitung("SELECT count(*) FROM foto").await, 1);

    // Tepat 500 KB diterima; 1 byte lebih ditolak (422).
    let batas = Uuid::now_v7().to_string();
    assert_eq!(
        unggah(&u, Some(&batas), &jpeg_uji(500 * 1024)).await.status,
        StatusCode::CREATED
    );
    let r = unggah(
        &u,
        Some(&Uuid::now_v7().to_string()),
        &jpeg_uji(500 * 1024 + 1),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.json);
    // Bukan JPEG, JPEG terpotong, tanpa uuid, uuid rusak, tanpa berkas.
    let png = b"\x89PNG\r\n\x1a\n....................".to_vec();
    assert_eq!(
        unggah(&u, Some(&Uuid::now_v7().to_string()), &png)
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let mut terpotong = jpeg_uji(100);
    terpotong.truncate(50);
    assert_eq!(
        unggah(&u, Some(&Uuid::now_v7().to_string()), &terpotong)
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        unggah(&u, None, &jpeg_uji(100)).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        unggah(&u, Some("bukan-uuid"), &jpeg_uji(100)).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = kirim_bytes(
        &u.app,
        "/v1/unggah-foto",
        &[("authorization", &u.sopir)],
        "application/json",
        b"{}".to_vec(),
    )
    .await;
    assert!(
        r.status.is_client_error() && r.status != StatusCode::UNAUTHORIZED,
        "{}",
        r.status
    );
    // Hanya sopir; tanpa token 401.
    let (ct, badan) = multipart_foto(Some(&Uuid::now_v7().to_string()), &jpeg_uji(100));
    assert_eq!(
        kirim_bytes(&u.app, "/v1/unggah-foto", &[], &ct, badan.clone())
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        kirim_bytes(&u.app, "/v1/unggah-foto", &u.sesi.h(), &ct, badan)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(u.hitung("SELECT count(*) FROM foto").await, 2);
}

#[sqlx::test]
async fn bukti_terima_maks_4_foto_dan_lapangan_menang(pool: PgPool) {
    let u = siapkan(pool).await;
    let trip = u.trip_siap().await;
    let mut foto = Vec::new();
    for _ in 0..5 {
        let id = Uuid::now_v7().to_string();
        assert_eq!(
            unggah(&u, Some(&id), &jpeg_uji(2_000)).await.status,
            StatusCode::CREATED
        );
        foto.push(id);
    }
    let bukti = |foto: &[String]| json!({"uuid": Uuid::now_v7(), "trip_id": trip, "titik_id": "PWN-1", "waktu": "2026-10-10T07:20:00+07:00", "foto_ids": foto, "catatan": "diterima Pak Budi"});

    // 5 foto ditolak, foto belum diunggah ditolak, tanpa foto dan tanda tangan ditolak.
    assert_eq!(
        u.sopir_post("/v1/bukti-terima", bukti(&foto)).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        u.sopir_post("/v1/bukti-terima", bukti(&[Uuid::now_v7().to_string()]))
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        u.sopir_post("/v1/bukti-terima", bukti(&[])).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        u.sopir_post(
            "/v1/bukti-terima",
            bukti(&[foto[0].clone(), foto[0].clone()])
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    // 4 foto diterima; kirim ulang: duplikat tanpa baris ganda.
    let b = bukti(&foto[..4]);
    let r = u.sopir_post("/v1/bukti-terima", b.clone()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let r = u.sopir_post("/v1/bukti-terima", b).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["duplikat"], true);
    assert_eq!(u.hitung("SELECT count(*) FROM bukti_terima").await, 1);
    // Keranjang titik itu sudah dikosongkan, yang titik lain belum.
    assert_eq!(
        u.hitung("SELECT count(*) FROM keranjang_isi WHERE selesai_at IS NOT NULL")
            .await,
        1
    );

    // Bukti baru dengan UUID lain untuk titik sama: lapangan menang, riwayat tersimpan, konflik tercatat.
    let r = u.sopir_post("/v1/bukti-terima", bukti(&foto[..1])).await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(u.hitung("SELECT count(*) FROM bukti_terima").await, 2);
    assert_eq!(
        u.hitung("SELECT count(*) FROM insiden WHERE jenis = 'konflik_sinkron'")
            .await,
        1
    );

    // Tanda tangan saja cukup; titik/trip tak ada 404; trip orang lain 403.
    let mut ttd = bukti(&[]);
    ttd["titik_id"] = json!("LPK-2");
    ttd["tanda_tangan"] = json!("TTD-0001");
    assert_eq!(
        u.sopir_post("/v1/bukti-terima", ttd).await.status,
        StatusCode::CREATED
    );
    let mut nol = bukti(&foto[..1]);
    nol["titik_id"] = json!("ZZZ-9");
    assert_eq!(
        u.sopir_post("/v1/bukti-terima", nol).await.status,
        StatusCode::NOT_FOUND
    );
    let lot: String = sqlx::query_scalar("SELECT status FROM lot")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(lot, "DITERIMA", "semua keranjang lot sudah selesai");
}

// ---------- insiden ----------

#[sqlx::test]
async fn insiden_ber_id_ins_tanggal_urut_idempoten_dan_validasi(pool: PgPool) {
    let u = siapkan(pool).await;
    let uuid = Uuid::now_v7();
    let r = u
        .sopir_post(
            "/v1/insiden",
            json!({"uuid": uuid, "jenis": "keterlambatan", "catatan": "macet"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["id"], format!("INS-{}-001", today_compact()));
    let r2 = u
        .sopir_post(
            "/v1/insiden",
            json!({"uuid": uuid, "jenis": "keterlambatan"}),
        )
        .await;
    assert_eq!(r2.status, StatusCode::OK);
    assert_eq!(
        r2.json,
        json!({"id": format!("INS-{}-001", today_compact()), "duplikat": true})
    );
    let r3 = u
        .sopir_post(
            "/v1/insiden",
            json!({"uuid": Uuid::now_v7(), "jenis": "ban bocor"}),
        )
        .await;
    assert_eq!(r3.json["id"], format!("INS-{}-002", today_compact()));
    assert_eq!(u.hitung("SELECT count(*) FROM insiden").await, 2);

    for badan in [
        json!({"uuid": Uuid::now_v7(), "jenis": "  "}),
        json!({"uuid": Uuid::now_v7(), "jenis": "x".repeat(65)}),
        json!({"uuid": Uuid::now_v7(), "jenis": "a", "catatan": "y".repeat(2001)}),
        json!({"uuid": Uuid::now_v7(), "jenis": "a", "foto_ids": [Uuid::now_v7()]}),
        json!({"uuid": Uuid::now_v7()}),
    ] {
        assert_eq!(
            u.sopir_post("/v1/insiden", badan.clone()).await.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{badan}"
        );
    }
    let _ = today();
}
