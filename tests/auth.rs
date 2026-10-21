//! Uji LMB-BE-003: JWT 15 menit, refresh + reuse-detection, kunci PIN, device key, service key.

mod common;

use axum::{http::StatusCode, routing::get, Router};
use chrono::{Duration, Utc};
use common::*;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use lumbung_backend_service::{
    auth::{
        jwt::Klaim,
        kunci,
        peran::Peran,
        principal::{AkunAdmin, Gudang, Layanan, Pengguna, PerangkatGudang},
    },
    state::AppState,
};
use serde_json::json;
use sqlx::PgPool;

/// Router uji dengan satu rute per ekstraktor untuk memeriksa matriks hak akses.
fn app_uji(state: &AppState) -> Router {
    let uji = Router::new()
        .route("/uji/pengguna", get(|_: Pengguna| async { "ok" }))
        .route("/uji/perangkat", get(|_: PerangkatGudang| async { "ok" }))
        .route("/uji/gudang", get(|_: Gudang| async { "ok" }))
        .route("/uji/layanan", get(|_: Layanan| async { "ok" }))
        .route("/uji/admin", get(|_: AkunAdmin| async { "ok" }))
        .with_state(state.clone());
    app(state).merge(uji)
}

async fn hitung(pool: &PgPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

// ---------- login + JWT ----------

#[sqlx::test]
async fn masuk_menerbitkan_jwt_15_menit_audiens_lumbung_dan_refresh_berhash(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;

    let r = kirim(
        &app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "sopir-02", "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["peran"], "sopir");

    let klaim = state
        .jwt
        .verifikasi(r.json["token"].as_str().unwrap())
        .expect("token sah");
    assert_eq!(klaim.aud, "lumbung");
    assert_eq!(klaim.exp - klaim.iat, 15 * 60);
    assert_eq!(klaim.akun, "sopir-02");

    // Refresh 7 hari; di DB hanya hash, bukan token mentah.
    let refresh = r.json["refresh_token"].as_str().unwrap();
    let rahasia = refresh.split_once('.').unwrap().1;
    let (hash, umur_hari): (String, f64) = sqlx::query_as(
        "SELECT token_hash, (EXTRACT(EPOCH FROM (expires_at - created_at)) / 86400.0)::float8 FROM refresh_token",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hash.len(), 64);
    assert_ne!(hash, rahasia);
    assert!((umur_hari - 7.0).abs() < 0.01, "umur refresh {umur_hari}");
}

#[sqlx::test]
async fn pin_salah_dan_akun_tak_ada_sama_sama_401(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "PTN-007", Peran::Petani).await;

    for badan in [
        json!({"akun": "PTN-007", "pin": "000000"}),
        json!({"akun": "tidak-ada", "pin": PIN_UJI}),
    ] {
        let r = kirim(&app, "POST", "/v1/auth/masuk", &[], Some(badan)).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert_eq!(r.json["error"], "tidak sah");
    }
}

#[sqlx::test]
async fn format_pin_salah_422_dan_json_rusak_400(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "PTN-007", Peran::Petani).await;

    let r = kirim(
        &app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "PTN-007", "pin": "12ab56"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = kirim(
        &app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "PTN-007"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    let r = kirim_mentah(&app, "POST", "/v1/auth/masuk", &[], Some("{rusak")).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn tiga_salah_pin_mengunci_15_menit(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let salah = json!({"akun": "sopir-02", "pin": "999999"});
    let benar = json!({"akun": "sopir-02", "pin": PIN_UJI});

    for _ in 0..3 {
        let r = kirim(&app, "POST", "/v1/auth/masuk", &[], Some(salah.clone())).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // Terkunci: PIN benar pun ditolak dengan 429.
    let r = kirim(&app, "POST", "/v1/auth/masuk", &[], Some(benar.clone())).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    let menit: f64 = sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM (terkunci_sampai - now())) / 60.0)::float8 FROM pengguna WHERE akun = 'sopir-02'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(menit > 14.0 && menit <= 15.0, "sisa kunci {menit} menit");

    // Kunci lewat: bisa masuk lagi.
    sqlx::query("UPDATE pengguna SET terkunci_sampai = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let r = kirim(&app, "POST", "/v1/auth/masuk", &[], Some(benar)).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[sqlx::test]
async fn pin_benar_mereset_hitungan_salah(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let salah = json!({"akun": "sopir-02", "pin": "999999"});
    let benar = json!({"akun": "sopir-02", "pin": PIN_UJI});

    for _ in 0..2 {
        kirim(&app, "POST", "/v1/auth/masuk", &[], Some(salah.clone())).await;
    }
    let r = kirim(&app, "POST", "/v1/auth/masuk", &[], Some(benar)).await;
    assert_eq!(r.status, StatusCode::OK);
    // Dua salah lagi belum cukup mengunci karena hitungan sudah direset.
    for _ in 0..2 {
        kirim(&app, "POST", "/v1/auth/masuk", &[], Some(salah.clone())).await;
    }
    assert_eq!(
        hitung(
            &pool,
            "SELECT count(*) FROM pengguna WHERE terkunci_sampai IS NOT NULL"
        )
        .await,
        0
    );
}

#[sqlx::test]
async fn akun_nonaktif_tidak_bisa_masuk_dan_tokennya_mati(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let (token, _) = masuk(&app, "sopir-02").await;
    let h = bearer(&token);
    let r = kirim(&app, "GET", "/uji/pengguna", &[("authorization", &h)], None).await;
    assert_eq!(r.status, StatusCode::OK);

    sqlx::query("UPDATE pengguna SET aktif = false")
        .execute(&pool)
        .await
        .unwrap();
    let r = kirim(&app, "GET", "/uji/pengguna", &[("authorization", &h)], None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "sopir-02", "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn token_kedaluwarsa_audiens_salah_dan_tanda_tangan_asing_ditolak(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);
    let id = buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;

    // Kedaluwarsa (umur negatif melewati toleransi 5 detik).
    let (kedaluwarsa, _) = state
        .jwt
        .terbitkan(id, "sopir-02", "sopir", None, false, Duration::minutes(-5))
        .unwrap();
    // Audiens salah, ditandatangani kunci asli.
    let (privat, _) = pasangan_kunci();
    let klaim = |aud: &str| Klaim {
        sub: id.to_string(),
        aud: aud.into(),
        iss: "lumbung-backend-service".into(),
        iat: Utc::now().timestamp(),
        exp: (Utc::now() + Duration::minutes(5)).timestamp(),
        jti: uuid::Uuid::now_v7().to_string(),
        akun: "sopir-02".into(),
        peran: "sopir".into(),
        fam: None,
        shift: false,
    };
    let tanda = |kunci_pem: &str, aud: &str| {
        jsonwebtoken::encode(
            &Header::new(Algorithm::EdDSA),
            &klaim(aud),
            &EncodingKey::from_ed_pem(kunci_pem.as_bytes()).unwrap(),
        )
        .unwrap()
    };
    let audiens_salah = tanda(&state.config.jwt_private_key, "pihak-lain");
    let tanda_tangan_asing = tanda(&privat, "lumbung"); // kunci bukan milik layanan
    let sah = tanda(&state.config.jwt_private_key, "lumbung");

    for (nama, token, harap) in [
        ("kedaluwarsa", kedaluwarsa, StatusCode::UNAUTHORIZED),
        ("audiens salah", audiens_salah, StatusCode::UNAUTHORIZED),
        ("kunci asing", tanda_tangan_asing, StatusCode::UNAUTHORIZED),
        ("sah", sah, StatusCode::OK),
        (
            "sampah",
            "bukan.jwt.sama-sekali".to_string(),
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let h = bearer(&token);
        let r = kirim(&app, "GET", "/uji/pengguna", &[("authorization", &h)], None).await;
        assert_eq!(r.status, harap, "kasus: {nama}");
    }
    // Tanpa header sama sekali.
    let r = kirim(&app, "GET", "/uji/pengguna", &[], None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

// ---------- refresh + reuse detection ----------

#[sqlx::test]
async fn refresh_merotasi_token_dan_token_lama_tidak_berlaku_ganda(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let (_, refresh1) = masuk(&app, "sopir-02").await;

    let r = kirim(
        &app,
        "POST",
        "/v1/auth/refresh",
        &[],
        Some(json!({"refresh_token": refresh1})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let refresh2 = r.json["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(refresh1, refresh2);
    assert!(state
        .jwt
        .verifikasi(r.json["token"].as_str().unwrap())
        .is_ok());
    // Keluarga sama setelah rotasi.
    assert_eq!(
        hitung(&pool, "SELECT count(DISTINCT family_id) FROM refresh_token").await,
        1
    );
}

#[sqlx::test]
async fn refresh_dipakai_ulang_mencabut_seluruh_keluarga_pengguna(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    buat_pengguna(&pool, "PTN-007", Peran::Petani).await;
    // Dua sesi (dua keluarga) milik sopir-02 + satu sesi pengguna lain.
    let (_, lama) = masuk(&app, "sopir-02").await;
    let (_, sesi_kedua) = masuk(&app, "sopir-02").await;
    let (_, milik_lain) = masuk(&app, "PTN-007").await;

    // Pakai pertama sah -> dapat token baru.
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/refresh",
        &[],
        Some(json!({"refresh_token": lama})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let baru = r.json["refresh_token"].as_str().unwrap().to_string();

    // Pakai ulang token lama -> 401 dan semua keluarga sopir-02 dicabut.
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/refresh",
        &[],
        Some(json!({"refresh_token": lama})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        hitung(
            &pool,
            "SELECT count(*) FROM audit_log WHERE aksi = 'refresh_dipakai_ulang'"
        )
        .await,
        1
    );

    for (nama, token) in [("token baru", baru), ("sesi kedua", sesi_kedua)] {
        let r = kirim(
            &app,
            "POST",
            "/v1/auth/refresh",
            &[],
            Some(json!({"refresh_token": token})),
        )
        .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{nama} harus dicabut");
    }
    // Pengguna lain tidak terdampak.
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/refresh",
        &[],
        Some(json!({"refresh_token": milik_lain})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[sqlx::test]
async fn refresh_kedaluwarsa_rahasia_salah_dan_format_rusak_401(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let (_, refresh) = masuk(&app, "sopir-02").await;
    let (id, _) = refresh.split_once('.').unwrap();

    for token in [
        format!("{id}.{}", "0".repeat(64)), // rahasia salah
        "tanpa-titik".to_string(),
        "bukan-uuid.abc".to_string(),
        format!("{}.{}", uuid::Uuid::nil(), "a".repeat(64)), // id tak ada
    ] {
        let r = kirim(
            &app,
            "POST",
            "/v1/auth/refresh",
            &[],
            Some(json!({"refresh_token": token})),
        )
        .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "token: {token}");
    }
    // Rahasia salah tidak boleh menghanguskan token asli.
    sqlx::query("UPDATE refresh_token SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/refresh",
        &[],
        Some(json!({"refresh_token": refresh})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "refresh kedaluwarsa");
}

#[sqlx::test]
async fn keluar_menghapus_keluarga_refresh(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let (token, refresh) = masuk(&app, "sopir-02").await;
    let (_, sesi_lain) = masuk(&app, "sopir-02").await;
    assert_eq!(hitung(&pool, "SELECT count(*) FROM refresh_token").await, 2);

    let h = bearer(&token);
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/keluar",
        &[("authorization", &h)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    // Hanya keluarga sesi ini yang dihapus.
    assert_eq!(hitung(&pool, "SELECT count(*) FROM refresh_token").await, 1);

    let r = kirim(
        &app,
        "POST",
        "/v1/auth/refresh",
        &[],
        Some(json!({"refresh_token": refresh})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/refresh",
        &[],
        Some(json!({"refresh_token": sesi_lain})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    // Keluar tanpa token = 401.
    let r = kirim(&app, "POST", "/v1/auth/keluar", &[], None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

// ---------- device key ----------

#[sqlx::test]
async fn device_key_sah_diterima_salah_atau_kosong_ditolak(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);

    let r = kirim(
        &app,
        "GET",
        "/uji/perangkat",
        &[("x-device-key", KUNCI_GUDANG)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    for header in [
        vec![("x-device-key", "kunci-salah")],
        vec![("x-device-key", "")],
        vec![],
    ] {
        let r = kirim(&app, "GET", "/uji/perangkat", &header, None).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
}

#[sqlx::test]
async fn device_key_tidak_boleh_membaca_data_pengguna(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);
    let kunci = [("x-device-key", KUNCI_GUDANG)];

    // Endpoint bearer-only, termasuk keluar dan sesi gudang tanpa bearer: semua 401.
    for (metode, uri) in [
        ("GET", "/uji/pengguna"),
        ("GET", "/uji/gudang"),
        ("GET", "/uji/admin"),
        ("POST", "/v1/auth/keluar"),
        ("POST", "/v1/gudang/keluar"),
    ] {
        let r = kirim(&app, metode, uri, &kunci, None).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{metode} {uri}");
    }
    assert_eq!(
        kunci::HAK_PERANGKAT,
        &["timbangan:tulis", "fee.status_cetak:tulis"]
    );
}

#[sqlx::test]
async fn rotasi_90_hari_dan_tenggang_7_hari(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);

    // Kunci awal berlaku 90 hari.
    let sisa_hari: f64 = sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM (berlaku_sampai - dibuat_at)) / 86400.0)::float8 FROM kunci_perangkat",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!((sisa_hari - 90.0).abs() < 0.01, "masa berlaku {sisa_hari}");

    // Rotasi: kunci baru berlaku, kunci lama masih berlaku <= 7 hari.
    kunci::rotasi(&pool, "GUDANG", "kunci-gudang-baru-0002")
        .await
        .unwrap();
    let kunci_lama = [("x-device-key", KUNCI_GUDANG)];
    let kunci_baru = [("x-device-key", "kunci-gudang-baru-0002")];
    assert_eq!(
        kirim(&app, "GET", "/uji/perangkat", &kunci_baru, None)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        kirim(&app, "GET", "/uji/perangkat", &kunci_lama, None)
            .await
            .status,
        StatusCode::OK,
        "kunci lama harus hidup dalam masa tenggang"
    );
    let tenggang: f64 = sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM (berlaku_sampai - now())) / 86400.0)::float8
         FROM kunci_perangkat WHERE key_hash = encode(digest($1, 'sha256'), 'hex')",
    )
    .bind(KUNCI_GUDANG)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        tenggang > 6.9 && tenggang <= 7.0,
        "tenggang {tenggang} hari"
    );

    // Lewat tenggang: kunci lama mati, kunci baru tetap hidup.
    sqlx::query(
        "UPDATE kunci_perangkat SET berlaku_sampai = now() - interval '1 second'
         WHERE key_hash = encode(digest($1, 'sha256'), 'hex')",
    )
    .bind(KUNCI_GUDANG)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        kirim(&app, "GET", "/uji/perangkat", &kunci_lama, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        kirim(&app, "GET", "/uji/perangkat", &kunci_baru, None)
            .await
            .status,
        StatusCode::OK
    );
}

#[sqlx::test]
async fn kunci_tanpa_rotasi_mati_setelah_90_hari_dan_bisa_dicabut(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);
    let kunci_hdr = [("x-device-key", KUNCI_GUDANG)];

    // Pencabutan seketika (perangkat hilang).
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM kunci_perangkat")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(kunci::cabut(&pool, id).await.unwrap());
    assert!(
        !kunci::cabut(&pool, id).await.unwrap(),
        "pencabutan idempoten"
    );
    assert_eq!(
        kirim(&app, "GET", "/uji/perangkat", &kunci_hdr, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    // pastikan_kunci_awal idempoten untuk kunci yang sudah dikenal.
    kunci::pastikan_kunci_awal(&pool, "GUDANG", KUNCI_GUDANG)
        .await
        .unwrap();
    assert_eq!(
        hitung(&pool, "SELECT count(*) FROM kunci_perangkat").await,
        1
    );
}

// ---------- login shift gudang ----------

#[sqlx::test]
async fn shift_gudang_masuk_12_jam_dan_keluar_mencabut_token(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);
    buat_pengguna(&pool, "OPR-SORTIR-1", Peran::AdminGudang).await;
    let kunci_hdr = ("x-device-key", KUNCI_GUDANG);

    let r = kirim(
        &app,
        "POST",
        "/v1/gudang/masuk",
        &[kunci_hdr],
        Some(json!({"operator": "OPR-SORTIR-1", "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let token = r.json["token"].as_str().unwrap().to_string();
    let klaim = state.jwt.verifikasi(&token).unwrap();
    assert!(klaim.shift);
    assert_eq!(klaim.exp - klaim.iat, 12 * 3600);

    let h = bearer(&token);
    // Token shift + kunci perangkat membuka endpoint gudang; tanpa kunci perangkat ditolak.
    let r = kirim(
        &app,
        "GET",
        "/uji/gudang",
        &[("authorization", &h), kunci_hdr],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = kirim(&app, "GET", "/uji/gudang", &[("authorization", &h)], None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    let r = kirim(
        &app,
        "POST",
        "/v1/gudang/keluar",
        &[("authorization", &h), kunci_hdr],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = kirim(
        &app,
        "GET",
        "/uji/gudang",
        &[("authorization", &h), kunci_hdr],
        None,
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNAUTHORIZED,
        "token shift harus dicabut"
    );
}

#[sqlx::test]
async fn shift_gudang_wajib_device_key_pin_benar_dan_peran_operator(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "OPR-SORTIR-1", Peran::AdminGudang).await;
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    let badan = |akun: &str, pin: &str| json!({"operator": akun, "pin": pin});

    // Tanpa / salah kunci perangkat: 401.
    let r = kirim(
        &app,
        "POST",
        "/v1/gudang/masuk",
        &[],
        Some(badan("OPR-SORTIR-1", PIN_UJI)),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = kirim(
        &app,
        "POST",
        "/v1/gudang/masuk",
        &[("x-device-key", "salah")],
        Some(badan("OPR-SORTIR-1", PIN_UJI)),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Sopir tidak boleh membuka shift gudang: 403 + audit.
    let kunci_hdr = [("x-device-key", KUNCI_GUDANG)];
    let r = kirim(
        &app,
        "POST",
        "/v1/gudang/masuk",
        &kunci_hdr,
        Some(badan("sopir-02", PIN_UJI)),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(
        hitung(
            &pool,
            "SELECT count(*) FROM audit_log WHERE aksi = 'akses_ditolak' AND aktor = 'sopir-02'"
        )
        .await,
        1
    );

    // Tiga salah PIN operator mengunci 15 menit dan tercatat untuk Kepala Agregasi.
    for _ in 0..3 {
        let r = kirim(
            &app,
            "POST",
            "/v1/gudang/masuk",
            &kunci_hdr,
            Some(badan("OPR-SORTIR-1", "000000")),
        )
        .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let r = kirim(
        &app,
        "POST",
        "/v1/gudang/masuk",
        &kunci_hdr,
        Some(badan("OPR-SORTIR-1", PIN_UJI)),
    )
    .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        hitung(
            &pool,
            "SELECT count(*) FROM audit_log WHERE aksi = 'akun_terkunci'"
        )
        .await,
        1
    );
}

#[sqlx::test]
async fn token_akses_biasa_tidak_bisa_menutup_shift(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "OPR-SORTIR-1", Peran::AdminGudang).await;
    let (token, _) = masuk(&app, "OPR-SORTIR-1").await;
    let h = bearer(&token);
    let r = kirim(
        &app,
        "POST",
        "/v1/gudang/keluar",
        &[("authorization", &h), ("x-device-key", KUNCI_GUDANG)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

// ---------- service key + peran ----------

#[sqlx::test]
async fn service_key_hanya_untuk_jalur_layanan_dan_admin(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);
    let layanan = [("x-service-key", KUNCI_LAYANAN)];

    for (uri, harap) in [
        ("/uji/layanan", StatusCode::OK),
        ("/uji/admin", StatusCode::OK),
        // Tanpa akses auth pengguna atau endpoint gudang.
        ("/uji/pengguna", StatusCode::UNAUTHORIZED),
        ("/uji/gudang", StatusCode::UNAUTHORIZED),
        ("/uji/perangkat", StatusCode::UNAUTHORIZED),
    ] {
        let r = kirim(&app, "GET", uri, &layanan, None).await;
        assert_eq!(r.status, harap, "{uri}");
    }
    let r = kirim(&app, "POST", "/v1/auth/keluar", &layanan, None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    for kunci_salah in ["salah", "", KUNCI_GUDANG] {
        let r = kirim(
            &app,
            "GET",
            "/uji/layanan",
            &[("x-service-key", kunci_salah)],
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "kunci {kunci_salah:?}");
    }
    // Kunci perangkat tidak menggantikan kunci layanan.
    let r = kirim(
        &app,
        "GET",
        "/uji/layanan",
        &[("x-device-key", KUNCI_GUDANG)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn peran_salah_mendapat_403_tercatat_dan_admin_lolos(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app_uji(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    buat_pengguna(&pool, "PTN-007", Peran::Petani).await;
    buat_pengguna(&pool, "adm-01", Peran::AdminKeuangan).await;

    for akun in ["sopir-02", "PTN-007"] {
        let (token, _) = masuk(&app, akun).await;
        let h = bearer(&token);
        let r = kirim(
            &app,
            "GET",
            "/uji/admin",
            &[("authorization", &h), ("x-perangkat-id", "HP-1")],
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{akun}");
        assert_eq!(r.json["error"], "akses ditolak");
    }
    assert_eq!(
        hitung(
            &pool,
            "SELECT count(*) FROM audit_log WHERE aksi = 'akses_ditolak' AND perangkat = 'HP-1'"
        )
        .await,
        2
    );

    let (token, _) = masuk(&app, "adm-01").await;
    let h = bearer(&token);
    let r = kirim(&app, "GET", "/uji/admin", &[("authorization", &h)], None).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[sqlx::test]
async fn galat_database_tidak_membocorkan_detail(pool: PgPool) {
    let state = state_uji(pool.clone()).await;
    let app = app(&state);
    buat_pengguna(&pool, "sopir-02", Peran::Sopir).await;
    // Rusak skema sengaja agar query login gagal.
    sqlx::query("ALTER TABLE pengguna RENAME COLUMN pin_hash TO pin_hash_rusak")
        .execute(&pool)
        .await
        .unwrap();
    let r = kirim(
        &app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(json!({"akun": "sopir-02", "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(r.json["error"], "galat internal");
}

#[sqlx::test]
async fn nomor_audit_berformat_dan_menaik_per_hari(pool: PgPool) {
    use lumbung_backend_service::audit;
    let mut conn = pool.acquire().await.unwrap();
    let a = audit::catat_beraudit(&mut conn, "adm-01", "uji", None, json!({}))
        .await
        .unwrap();
    let b = audit::catat_beraudit(&mut conn, "adm-01", "uji", None, json!({}))
        .await
        .unwrap();
    assert!(a.starts_with("AUD-") && a.ends_with("-001"), "{a}");
    assert!(b.ends_with("-002"), "{b}");
    assert_eq!(a.len(), "AUD-20261009-003".len());
}
