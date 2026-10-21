//! Uji LMB-BE-007: papan harga mingguan, publik cache 15 menit, petani read-only, kesehatan.

mod common;

use axum::http::StatusCode;
use chrono::{Datelike, Duration, NaiveDate};
use common::*;
use lumbung_backend_service::{auth::peran::Peran, waktu};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// Senin pada minggu berjalan (WIB) ditambah `minggu` minggu.
fn senin(minggu: i64) -> NaiveDate {
    let hari_ini = waktu::hari_ini_wib();
    hari_ini - Duration::days(hari_ini.weekday().num_days_from_monday() as i64)
        + Duration::weeks(minggu)
}

/// Referensi yang menghasilkan contoh BR-305 (cabai 31500/26500, bawang 28000/23500, kangkung 6000/5000).
fn referensi_contoh() -> Value {
    json!([
        {"komoditas": "cabai", "harga": [35000, 36000, 35000]},
        {"komoditas": "bawang", "harga": [31200, 30000, 33000]},
        {"komoditas": "kangkung", "harga": [7000, 7500, 6800]},
    ])
}

fn badan(uuid: Uuid, mulai: NaiveDate) -> Value {
    json!({"uuid": uuid, "berlaku_mulai": mulai.to_string(), "referensi": referensi_contoh()})
}

async fn admin_token(app: &axum::Router, pool: &PgPool) -> String {
    buat_pengguna(pool, "adm-gudang", Peran::AdminGudang).await;
    bearer(&masuk(app, "adm-gudang").await.0)
}

#[sqlx::test]
async fn kesehatan_ok_dengan_waktu_server_tanpa_login(pool: PgPool) {
    let state = state_uji(pool).await;
    let app = app(&state);
    let r = kirim(&app, "GET", "/v1/kesehatan", &[], None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["status"], "ok");
    let waktu_server = r.json["waktu_server"].as_str().unwrap();
    let t = chrono::DateTime::parse_from_rfc3339(waktu_server).expect("RFC 3339");
    assert_eq!(t.offset().local_minus_utc(), 7 * 3600, "offset WIB");
    assert!(
        (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
            .num_seconds()
            .abs()
            < 5
    );
}

#[sqlx::test]
async fn admin_menerbitkan_papan_dan_publik_membacanya_persis_format_kontrak(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let h = admin_token(&app, &pool).await;
    let mulai = senin(0);

    let r = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(Uuid::now_v7(), mulai)),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let audit_id = r.json["audit_id"].as_str().unwrap();
    assert!(
        audit_id.len() == 16
            && audit_id.starts_with("AUD-")
            && audit_id[4..12].bytes().all(|b| b.is_ascii_digit()),
        "format audit_id: {audit_id}"
    );
    assert_eq!(r.json["duplikat"], false);

    // Publik tanpa login, tanpa header apa pun.
    let r = kirim(&app, "GET", "/v1/publik/papan-harga", &[], None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.headers.get("cache-control").unwrap(),
        "public, max-age=900"
    );
    let selesai = mulai + Duration::days(6);
    assert_eq!(
        r.json,
        json!({
            "cabai": {"a": 31500, "b": 26500},
            "bawang": {"a": 28000, "b": 23500},
            "kangkung": {"a": 6000, "b": 5000},
            "fee": {"dekat": 1500, "jauh": 2500},
            "berlaku": format!("{mulai} s.d. {selesai}"),
        })
    );

    // Lembar kerja referensi tersimpan dan penulisan beraudit (nama + waktu).
    let (n_ref, aktor): (i64, String) = sqlx::query_as(
        "SELECT jsonb_array_length(referensi)::bigint, diterbitkan_oleh FROM papan_harga",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n_ref, 3);
    assert_eq!(aktor, "adm-gudang");
    let audit_aktor: String = sqlx::query_scalar("SELECT aktor FROM audit_log WHERE audit_id = $1")
        .bind(audit_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audit_aktor, "adm-gudang");
}

#[sqlx::test]
async fn kirim_ulang_uuid_sama_200_duplikat_tanpa_rekaman_ganda(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let h = admin_token(&app, &pool).await;
    let uuid = Uuid::now_v7();
    let mulai = senin(0);

    let pertama = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(uuid, mulai)),
    )
    .await;
    assert_eq!(pertama.status, StatusCode::CREATED);
    let ulang = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(uuid, mulai)),
    )
    .await;
    assert_eq!(ulang.status, StatusCode::OK);
    assert_eq!(ulang.json["duplikat"], true);
    assert_eq!(ulang.json["audit_id"], pertama.json["audit_id"]);

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM papan_harga")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE aksi = 'papan_harga_terbit'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1, "duplikat tidak menambah audit");
}

#[sqlx::test]
async fn papan_kedua_untuk_minggu_sama_409(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let h = admin_token(&app, &pool).await;
    let mulai = senin(0);
    let a = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(Uuid::now_v7(), mulai)),
    )
    .await;
    assert_eq!(a.status, StatusCode::CREATED);
    let b = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(Uuid::now_v7(), mulai)),
    )
    .await;
    assert_eq!(b.status, StatusCode::CONFLICT);
    // Transaksi gagal tidak meninggalkan audit yatim.
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE aksi = 'papan_harga_terbit'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn validasi_422_untuk_hari_bukan_senin_referensi_kurang_dan_angka_buruk(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let h = admin_token(&app, &pool).await;
    let post = |b: Value| {
        let app = app.clone();
        let h = h.clone();
        async move {
            kirim(
                &app,
                "POST",
                "/v1/admin/papan-harga",
                &[("authorization", &h)],
                Some(b),
            )
            .await
        }
    };
    let ok = badan(Uuid::now_v7(), senin(0));

    // Bukan Senin.
    let mut b = ok.clone();
    b["berlaku_mulai"] = json!((senin(0) + Duration::days(1)).to_string());
    assert_eq!(post(b).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Komoditas kurang / dobel / harga bukan 3 / harga <= 0 / komoditas tak dikenal.
    for referensi in [
        json!([{"komoditas": "cabai", "harga": [1000, 1000, 1000]}]),
        json!([
            {"komoditas": "cabai", "harga": [1000, 1000, 1000]},
            {"komoditas": "cabai", "harga": [1000, 1000, 1000]},
            {"komoditas": "bawang", "harga": [1000, 1000, 1000]},
        ]),
        json!([
            {"komoditas": "cabai", "harga": [1000, 1000]},
            {"komoditas": "bawang", "harga": [1000, 1000, 1000]},
            {"komoditas": "kangkung", "harga": [1000, 1000, 1000]},
        ]),
        json!([
            {"komoditas": "cabai", "harga": [1000, 0, 1000]},
            {"komoditas": "bawang", "harga": [1000, 1000, 1000]},
            {"komoditas": "kangkung", "harga": [1000, 1000, 1000]},
        ]),
        json!([
            {"komoditas": "cabai", "harga": [1000, -5, 1000]},
            {"komoditas": "bawang", "harga": [1000, 1000, 1000]},
            {"komoditas": "kangkung", "harga": [1000, 1000, 1000]},
        ]),
        json!([
            {"komoditas": "tomat", "harga": [1000, 1000, 1000]},
            {"komoditas": "bawang", "harga": [1000, 1000, 1000]},
            {"komoditas": "kangkung", "harga": [1000, 1000, 1000]},
        ]),
    ] {
        let mut b = ok.clone();
        b["referensi"] = referensi.clone();
        let r = post(b).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{referensi}");
    }

    // Klien dilarang mengirim angka hitungan server (fee/harga/grade).
    for tambahan in ["fee", "harga", "grade"] {
        let mut b = ok.clone();
        b[tambahan] = json!({"dekat": 1});
        assert_eq!(
            post(b).await.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{tambahan}"
        );
    }
    // Tidak ada yang tersimpan.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM papan_harga")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn hanya_admin_gudang_atau_kunci_layanan_boleh_menerbitkan(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    buat_pengguna(&pool, "PTN-007", Peran::Petani).await;
    buat_pengguna(&pool, "adm-keu", Peran::AdminKeuangan).await;

    for akun in ["sopir-02", "PTN-007", "adm-keu"] {
        let h = bearer(&masuk(&app, akun).await.0);
        let r = kirim(
            &app,
            "POST",
            "/v1/admin/papan-harga",
            &[("authorization", &h)],
            Some(badan(Uuid::now_v7(), senin(0))),
        )
        .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{akun}");
    }
    // Tanpa kredensial dan kunci perangkat saja: 401.
    let b = badan(Uuid::now_v7(), senin(0));
    let r = kirim(&app, "POST", "/v1/admin/papan-harga", &[], Some(b.clone())).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("x-device-key", KUNCI_GUDANG)],
        Some(b.clone()),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Kunci layanan (web SvelteKit) diterima; aktor audit = layanan-web.
    let r = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("x-service-key", KUNCI_LAYANAN)],
        Some(b),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    let aktor: String = sqlx::query_scalar("SELECT diterbitkan_oleh FROM papan_harga")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(aktor, "layanan-web");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM papan_harga")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "403/401 tidak menyimpan apa pun");
}

#[sqlx::test]
async fn publik_404_bila_belum_ada_papan_minggu_berjalan(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let h = admin_token(&app, &pool).await;

    assert_eq!(
        kirim(&app, "GET", "/v1/publik/papan-harga", &[], None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // Papan untuk minggu depan belum berlaku hari ini.
    let r = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(Uuid::now_v7(), senin(1))),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(
        kirim(&app, "GET", "/v1/publik/papan-harga", &[], None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // Papan minggu lalu sudah kedaluwarsa.
    let r = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(Uuid::now_v7(), senin(-1))),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(
        kirim(&app, "GET", "/v1/publik/papan-harga", &[], None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // Minggu berjalan terbit: publik melihatnya, bukan minggu lain.
    let r = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(Uuid::now_v7(), senin(0))),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = kirim(&app, "GET", "/v1/publik/papan-harga", &[], None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.json["berlaku"]
        .as_str()
        .unwrap()
        .starts_with(&senin(0).to_string()));
}

#[sqlx::test]
async fn petani_membaca_papan_read_only_dan_tidak_bisa_menulis(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    let h_admin = admin_token(&app, &pool).await;
    buat_pengguna(&pool, "PTN-007", Peran::Petani).await;
    let h = bearer(&masuk(&app, "PTN-007").await.0);

    // Tanpa login: 401 (hanya jalur publik yang terbuka).
    assert_eq!(
        kirim(&app, "GET", "/v1/papan-harga", &[], None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h_admin)],
        Some(badan(Uuid::now_v7(), senin(0))),
    )
    .await;
    let r = kirim(
        &app,
        "GET",
        "/v1/papan-harga",
        &[("authorization", &h)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["cabai"], json!({"a": 31500, "b": 26500}));
    assert_eq!(r.json["fee"], json!({"dekat": 1500, "jauh": 2500}));
    let cc = r.headers.get("cache-control").unwrap().to_str().unwrap();
    assert!(
        cc.starts_with("private"),
        "papan petani tidak boleh di-cache publik: {cc}"
    );

    // Petani tidak bisa mengubah harga: PUT/POST/DELETE tidak ada atau ditolak.
    let r = kirim(
        &app,
        "POST",
        "/v1/papan-harga",
        &[("authorization", &h)],
        Some(json!({})),
    )
    .await;
    assert_eq!(r.status, StatusCode::METHOD_NOT_ALLOWED);
    let r = kirim(
        &app,
        "POST",
        "/v1/admin/papan-harga",
        &[("authorization", &h)],
        Some(badan(Uuid::now_v7(), senin(1))),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn fee_selalu_dari_server_dan_tidak_bisa_diubah_lewat_skema(pool: PgPool) {
    // Skema DB menolak papan tanpa rentang 7 hari atau awal bukan Senin (pagar terakhir).
    let state = state_uji(pool.clone()).await;
    let _ = app(&state);
    let salah_hari = sqlx::query(
        "INSERT INTO papan_harga (uuid_klien, berlaku_mulai, berlaku_selesai, fee_dekat, fee_jauh, referensi, audit_id, diterbitkan_oleh)
         VALUES (gen_random_uuid(), '2026-10-07', '2026-10-13', 1500, 2500, '[]', 'x', 'x')",
    )
    .execute(&pool)
    .await;
    assert!(
        salah_hari.is_err(),
        "awal minggu bukan Senin harus ditolak DB"
    );
    let rentang_salah = sqlx::query(
        "INSERT INTO papan_harga (uuid_klien, berlaku_mulai, berlaku_selesai, fee_dekat, fee_jauh, referensi, audit_id, diterbitkan_oleh)
         VALUES (gen_random_uuid(), '2026-10-05', '2026-10-20', 1500, 2500, '[]', 'x', 'x')",
    )
    .execute(&pool)
    .await;
    assert!(
        rentang_salah.is_err(),
        "rentang bukan 7 hari harus ditolak DB"
    );
}
