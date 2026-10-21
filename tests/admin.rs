//! Uji endpoint baca admin, profil publik, 501 terdokumentasi, 404/405 JSON, kredensial rusak.

mod common;

use axum::http::StatusCode;
use common::*;
use lumbung_backend_service::{auth::peran::Peran, waktu};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test]
async fn endpoint_baca_admin_profil_501_dan_galat_json_baku(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let sesi = masuk_gudang(&app, &pool).await;
    buat_petani(&pool, "PTN-007").await;
    let layanan = [("x-service-key", KUNCI_LAYANAN)];
    let hari = waktu::hari_ini_wib().to_string();

    // Data: lot 120.000 g tersortir 100.000 + 14.000 + 6.000 hari ini; satu insiden.
    let r = kirim(&app, "POST", "/v1/lot", &sesi.h(), Some(json!({"uuid": Uuid::now_v7(), "kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": hari, "berat_kotor_g": 120_000}))).await;
    let kode = r.json["kode"].as_str().unwrap().to_string();
    kirim(&app, "POST", &format!("/v1/lot/{kode}/sortir"), &sesi.h(), Some(json!({"uuid": Uuid::now_v7(), "lot_id": kode, "grade_a_g": 100_000, "grade_b_g": 14_000, "grade_c_g": 6_000}))).await;

    let r = kirim(
        &app,
        "GET",
        &format!("/v1/admin/rekap-harian?tanggal={hari}"),
        &layanan,
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(
        r.json["masuk"],
        json!([
        {"komoditas": "cabai", "berat_g": 120_000}, {"komoditas": "bawang", "berat_g": 0}, {"komoditas": "kangkung", "berat_g": 0}])
    );
    assert_eq!(r.json["fee_tertagih"], 0);
    assert_eq!(r.json["susut_persen"], 0.0);
    for uri in [
        "/v1/admin/rekap-harian",
        "/v1/admin/rekap-harian?tanggal=kemarin",
    ] {
        assert_eq!(
            kirim(&app, "GET", uri, &layanan, None).await.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{uri}"
        );
    }

    let r = kirim(&app, "GET", "/v1/admin/mitra", &layanan, None).await;
    assert_eq!(
        r.json["data"],
        json!([{"kode": "PTN-007", "nama": "PTN-007", "status": "AKTIF"}])
    );

    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let sopir = bearer(&masuk(&app, "sopir-02").await.0);
    kirim(
        &app,
        "POST",
        "/v1/insiden",
        &[("authorization", &sopir)],
        Some(json!({"uuid": Uuid::now_v7(), "jenis": "keterlambatan", "catatan": "macet"})),
    )
    .await;
    let r = kirim(&app, "GET", "/v1/admin/insiden", &layanan, None).await;
    assert_eq!(r.json["data"].as_array().unwrap().len(), 1);
    assert_eq!(r.json["data"][0]["jenis"], "keterlambatan");
    assert!(r.json["data"][0]["waktu"]
        .as_str()
        .unwrap()
        .ends_with("+07:00"));
    // Sopir tidak boleh membaca endpoint admin.
    assert_eq!(
        kirim(
            &app,
            "GET",
            "/v1/admin/mitra",
            &[("authorization", &sopir)],
            None
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    let r = kirim(&app, "GET", "/v1/publik/profil", &[], None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json, json!({"nama": "Lumbung"}));
    sqlx::query("UPDATE profil_publik SET deskripsi = 'Agregasi panen', kontak_kemitraan = 'mitra@lumbung.id'")
        .execute(&pool)
        .await
        .unwrap();
    let r = kirim(&app, "GET", "/v1/publik/profil", &[], None).await;
    assert_eq!(
        r.json,
        json!({"nama": "Lumbung", "deskripsi": "Agregasi panen", "kontak_kemitraan": "mitra@lumbung.id"})
    );
    assert_eq!(
        r.headers.get("cache-control").unwrap(),
        "public, max-age=900"
    );

    // 501 terdokumentasi setelah autentikasi; tanpa kredensial tetap 401.
    buat_pengguna(&pool, "PTN-007", Peran::Petani).await;
    let petani = bearer(&masuk(&app, "PTN-007").await.0);
    assert_eq!(
        kirim(
            &app,
            "GET",
            "/v1/kontrak-saya",
            &[("authorization", &petani)],
            None
        )
        .await
        .status,
        StatusCode::NOT_IMPLEMENTED
    );
    assert_eq!(
        kirim(&app, "GET", "/v1/kontrak-saya", &[], None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        kirim(
            &app,
            "POST",
            "/v1/admin/mitra/PTN-007/status",
            &layanan,
            Some(json!({"status": "AKTIF"}))
        )
        .await
        .status,
        StatusCode::NOT_IMPLEMENTED
    );

    // 404/405 berbadan JSON baku.
    let r = kirim(&app, "GET", "/v1/tidak-ada", &[], None).await;
    assert_eq!(
        (r.status, r.json["error"].as_str()),
        (StatusCode::NOT_FOUND, Some("tidak ditemukan"))
    );
    let r = kirim(&app, "TRACE", "/v1/kesehatan", &[], None).await;
    assert_eq!(r.status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(r.json["error"].is_string());

    // Bearer rusak di samping kunci layanan sah: 401, tidak diam-diam diabaikan.
    let r = kirim(
        &app,
        "GET",
        "/v1/admin/mitra",
        &[
            ("x-service-key", KUNCI_LAYANAN),
            ("authorization", "Bearer rusak"),
        ],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}
