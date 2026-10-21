//! Uji LMB-BE-008: tulis tarif fee (BR-306), audit, peran, dan pemakaian tarif baru oleh fee dan papan.

mod common;

use axum::http::StatusCode;
use chrono::{Datelike, Duration};
use common::*;
use lumbung_backend_service::{auth::peran::Peran, invoice::senin_dari, waktu};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

fn badan(tanggal: &str, dekat: Value, jauh: Value) -> Value {
    json!({"uuid": Uuid::now_v7(), "berlaku_mulai": tanggal, "dekat": dekat, "jauh": jauh})
}

async fn post(app: &axum::Router, header: &[(&str, &str)], b: Value) -> Resp {
    kirim(app, "POST", "/v1/admin/tarif-fee", header, Some(b)).await
}

#[sqlx::test]
async fn tarif_awal_jadwal_baru_beraudit_idempoten_dan_aturan_br306(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let layanan = [("x-service-key", KUNCI_LAYANAN)];

    let r = kirim(&app, "GET", "/v1/admin/tarif-fee", &layanan, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["berlaku"], json!({"dekat": 1500, "jauh": 2500}));
    assert_eq!(r.json["riwayat"].as_array().unwrap().len(), 1);

    // Sah: 10 persen naik tepat, jauh di masa depan (semester 1 tahun 2099), beraudit.
    let b = badan("2099-02-01", json!(1650), json!(2750));
    let r = post(&app, &layanan, b.clone()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let audit_id = r.json["audit_id"].as_str().unwrap().to_string();
    assert!(audit_id.starts_with("AUD-") && audit_id.len() == 16);
    assert_eq!(r.json["duplikat"], false);
    let aktor: String = sqlx::query_scalar("SELECT aktor FROM audit_log WHERE audit_id = $1")
        .bind(&audit_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(aktor, "layanan-web");
    // Idempoten.
    let r = post(&app, &layanan, b).await;
    assert_eq!(
        (r.status, r.json["duplikat"].clone()),
        (StatusCode::OK, json!(true))
    );
    assert_eq!(r.json["audit_id"], audit_id.as_str());
    // Tarif hari ini belum berubah; riwayat memuat jadwal.
    let r = kirim(&app, "GET", "/v1/admin/tarif-fee", &layanan, None).await;
    assert_eq!(r.json["berlaku"], json!({"dekat": 1500, "jauh": 2500}));
    assert_eq!(r.json["riwayat"][0]["berlaku_mulai"], "2099-02-01");

    // Satu kali per semester: tanggal lain di semester yang sama 409; tanggal sama 409.
    assert_eq!(
        post(
            &app,
            &layanan,
            badan("2099-03-01", json!(1650), json!(2750))
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        post(
            &app,
            &layanan,
            badan("2099-02-01", json!(1650), json!(2750))
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    // Semester berikutnya boleh, 10 persen dihitung dari tarif yang berlaku saat itu (1650 -> 1815).
    assert_eq!(
        post(
            &app,
            &layanan,
            badan("2099-08-01", json!(1815), json!(3025))
        )
        .await
        .status,
        StatusCode::CREATED
    );
    assert_eq!(
        post(
            &app,
            &layanan,
            badan("2100-02-01", json!(1815), json!(3328))
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "3328 > 10% dari 3025"
    );
}

#[sqlx::test]
async fn validasi_422_pemberitahuan_30_hari_batas_10_persen_dan_angka(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let layanan = [("x-service-key", KUNCI_LAYANAN)];
    let hari = waktu::hari_ini_wib();
    let tgl = |n: i64| (hari + Duration::days(n)).to_string();

    for (nama, b) in [
        (
            "kurang dari 30 hari",
            badan(&tgl(29), json!(1500), json!(2500)),
        ),
        ("hari ini", badan(&tgl(0), json!(1500), json!(2500))),
        ("lampau", badan("2020-01-01", json!(1500), json!(2500))),
        (
            "naik lebih 10 persen dekat",
            badan("2099-02-01", json!(1651), json!(2500)),
        ),
        (
            "turun lebih 10 persen jauh",
            badan("2099-02-01", json!(1500), json!(2249)),
        ),
        ("nol", badan("2099-02-01", json!(0), json!(2500))),
        ("negatif", badan("2099-02-01", json!(-1500), json!(2500))),
        ("desimal", badan("2099-02-01", json!(1500.5), json!(2500))),
        ("teks", badan("2099-02-01", json!("1500"), json!(2500))),
        ("tanggal rusak", badan("kemarin", json!(1500), json!(2500))),
    ] {
        let r = post(&app, &layanan, b).await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{nama}: {}",
            r.json
        );
    }
    let mut asing = badan("2099-02-01", json!(1500), json!(2500));
    asing["fee"] = json!(1);
    assert_eq!(
        post(&app, &layanan, asing).await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    // Tepat 30 hari diterima.
    assert_eq!(
        post(&app, &layanan, badan(&tgl(30), json!(1500), json!(2500)))
            .await
            .status,
        StatusCode::CREATED
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tarif_fee")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2, "hanya yang sah tersimpan (awal + satu)");
}

#[sqlx::test]
async fn hanya_admin_gudang_atau_kunci_layanan_menulis_peran_lain_403(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let sesi = masuk_gudang(&app, &pool).await;
    for (akun, peran) in [
        ("sopir-02", Peran::Sopir),
        ("PTN-007", Peran::Petani),
        ("keu-01", Peran::AdminKeuangan),
        ("koord-01", Peran::KoordinatorTani),
    ] {
        buat_pengguna(&pool, akun, peran).await;
        let h = bearer(&masuk(&app, akun).await.0);
        let r = post(
            &app,
            &[("authorization", &h)],
            badan("2099-02-01", json!(1500), json!(2500)),
        )
        .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{akun}");
        // Membaca tarif boleh untuk admin mana pun, bukan sopir/petani.
        let g = kirim(
            &app,
            "GET",
            "/v1/admin/tarif-fee",
            &[("authorization", &h)],
            None,
        )
        .await;
        let harap = if peran.adalah_admin() {
            StatusCode::OK
        } else {
            StatusCode::FORBIDDEN
        };
        assert_eq!(g.status, harap, "GET {akun}");
    }
    assert_eq!(
        post(&app, &[], badan("2099-02-01", json!(1500), json!(2500)))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post(
            &app,
            &[("x-device-key", KUNCI_GUDANG)],
            badan("2099-02-01", json!(1500), json!(2500))
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tarif_fee")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "penolakan tidak menyimpan apa pun");
    // Admin gudang (token shift) boleh.
    let r = post(
        &app,
        &[("authorization", sesi.auth.as_str())],
        badan("2099-02-01", json!(1650), json!(2750)),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let aktor: String =
        sqlx::query_scalar("SELECT aktor FROM audit_log WHERE aksi = 'tarif_fee_terbit'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(aktor, "OPR-SORTIR-1");
}

#[sqlx::test]
async fn fee_dan_papan_memakai_tarif_yang_berlaku(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let sesi = masuk_gudang(&app, &pool).await;
    buat_petani(&pool, "PTN-007").await;
    // Tarif baru berlaku sejak awal minggu ini (disisipkan langsung agar melewati aturan 30 hari).
    let mulai = senin_dari(waktu::hari_ini_wib());
    sqlx::query("INSERT INTO tarif_fee (berlaku_mulai, dekat, jauh, diterbitkan_oleh) VALUES ($1, 1600, 2600, 'uji')")
        .bind(mulai)
        .execute(&pool)
        .await
        .unwrap();

    // Papan minggu ini memuat tarif baru pada `fee`.
    let r = kirim(&app, "POST", "/v1/admin/papan-harga", &[("x-service-key", KUNCI_LAYANAN)], Some(json!({"uuid": Uuid::now_v7(), "berlaku_mulai": mulai.to_string(), "referensi": [
        {"komoditas": "cabai", "harga": [35000, 36000, 35000]}, {"komoditas": "bawang", "harga": [31200, 30000, 33000]}, {"komoditas": "kangkung", "harga": [7000, 7500, 6800]}]}))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let r = kirim(&app, "GET", "/v1/publik/papan-harga", &[], None).await;
    assert_eq!(r.json["fee"], json!({"dekat": 1600, "jauh": 2600}));

    // Surat jalan: 170.000 g dekat x 1.600 = Rp272.000 (bukan 255.000).
    let l = kirim(&app, "POST", "/v1/lot", &sesi.h(), Some(json!({"uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": "2026-10-06", "berat_kotor_g": 1_000_000}))).await.json["kode"].as_str().unwrap().to_string();
    kirim(&app, "POST", &format!("/v1/lot/{l}/sortir"), &sesi.h(), Some(json!({"uuid": Uuid::now_v7(), "lot_id": l, "grade_a_g": 1_000_000, "grade_b_g": 0, "grade_c_g": 0}))).await;
    kirim(&app, "POST", "/v1/keranjang/batch", &sesi.h(), Some(json!({"uuid": Uuid::now_v7(), "keranjang": [{"kode": "KRJ-001", "lot_id": l, "grade": "A", "berat_g": 170_000}]}))).await;
    let t = kirim(&app, "POST", "/v1/trip", &sesi.h(), Some(json!({"uuid": Uuid::now_v7(), "rute": "UTARA", "kendaraan": "L 1", "keranjang": ["KRJ-001"],
        "titik": [{"id": "PWN-1", "zona": "DEKAT", "pesanan": "PSO-20261010-001", "pembeli": "PAWON-01", "keranjang": ["KRJ-001"]}]}))).await;
    let kode = t.json["kode"].as_str().unwrap().to_string();
    let r = kirim(
        &app,
        "POST",
        &format!("/v1/trip/{kode}/surat-jalan"),
        &sesi.h(),
        Some(json!({"uuid": Uuid::now_v7()})),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    assert_eq!(r.json["rincian_fee"][0]["tarif_per_kg"], 1600);
    assert_eq!(r.json["rincian_fee"][0]["nominal"], 272_000);
    assert_eq!(r.json["fee_total"], 272_000);
    let _ = hari_ini_tahun();
}

fn hari_ini_tahun() -> i32 {
    waktu::hari_ini_wib().year()
}
