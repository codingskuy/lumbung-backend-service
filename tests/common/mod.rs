//! Pembantu uji bersama: kunci ed25519 sementara, state uji, klien `oneshot`, seed akun.
#![allow(dead_code)]

use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, Method, Request, StatusCode},
    Router,
};
use lumbung_backend_service::{
    auth::{kunci, peran::Peran, pin},
    config::Config,
    router,
    state::AppState,
};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

pub const KUNCI_GUDANG: &str = "kunci-gudang-uji-0001";
pub const KUNCI_LAYANAN: &str = "kunci-layanan-uji-0001";
pub const PIN_UJI: &str = "123456";

const ALFABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut keluar = String::new();
    for potong in data.chunks(3) {
        let b = [
            potong[0],
            *potong.get(1).unwrap_or(&0),
            *potong.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        keluar.push(ALFABET[(n >> 18) as usize & 63] as char);
        keluar.push(ALFABET[(n >> 12) as usize & 63] as char);
        keluar.push(if potong.len() > 1 {
            ALFABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        keluar.push(if potong.len() > 2 {
            ALFABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    keluar
}

fn pem(label: &str, der: &[u8]) -> String {
    let isi = base64(der);
    let baris: Vec<&str> = isi
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        baris.join("\n")
    )
}

/// Pasangan kunci ed25519 baru (PEM privat PKCS#8, PEM publik SPKI); tidak ada rahasia di repo.
pub fn pasangan_kunci() -> (String, String) {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).expect("bangkitkan kunci");
    let pasangan = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("baca kunci");
    let mut spki = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    spki.extend_from_slice(pasangan.public_key().as_ref());
    (pem("PRIVATE KEY", pkcs8.as_ref()), pem("PUBLIC KEY", &spki))
}

/// State uji dengan kunci perangkat gudang terdaftar.
pub async fn state_uji(pool: PgPool) -> AppState {
    let (privat, publik) = pasangan_kunci();
    let config = Config {
        listen_addr: "127.0.0.1:0".into(),
        database_url: String::new(),
        jwt_private_key: privat,
        jwt_public_key: publik,
        device_api_key: KUNCI_GUDANG.into(),
        service_api_key: KUNCI_LAYANAN.into(),
    };
    kunci::pastikan_kunci_awal(&pool, "GUDANG", KUNCI_GUDANG)
        .await
        .expect("daftarkan kunci gudang");
    AppState::new(pool, config).expect("state uji")
}

/// Router layanan nyata.
pub fn app(state: &AppState) -> Router {
    router(state.clone())
}

/// Hasil panggilan HTTP.
pub struct Resp {
    pub status: StatusCode,
    pub json: Value,
    pub headers: HeaderMap,
}

/// Kirim permintaan; badan JSON opsional.
pub async fn kirim(
    app: &Router,
    metode: &str,
    uri: &str,
    header: &[(&str, &str)],
    badan: Option<Value>,
) -> Resp {
    let mentah = badan.map(|b| b.to_string());
    kirim_mentah(app, metode, uri, header, mentah.as_deref()).await
}

/// Kirim permintaan dengan badan teks mentah (untuk JSON rusak).
pub async fn kirim_mentah(
    app: &Router,
    metode: &str,
    uri: &str,
    header: &[(&str, &str)],
    badan: Option<&str>,
) -> Resp {
    let mut b = Request::builder()
        .method(Method::from_bytes(metode.as_bytes()).unwrap())
        .uri(uri);
    for (k, v) in header {
        b = b.header(*k, *v);
    }
    if badan.is_some() {
        b = b.header("content-type", "application/json");
    }
    let req = b
        .body(
            badan
                .map(|s| Body::from(s.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .unwrap();
    let res = app.clone().oneshot(req).await.expect("respons");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Resp {
        status,
        json,
        headers,
    }
}

/// Seed akun dengan PIN `PIN_UJI`; mengembalikan ID pengguna.
pub async fn buat_pengguna(pool: &PgPool, akun: &str, peran: Peran) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO pengguna (akun, nama, peran, pin_hash) VALUES ($1, $1, $2, $3) RETURNING id",
    )
    .bind(akun)
    .bind(peran.sebagai_str())
    .bind(pin::hash(PIN_UJI).unwrap())
    .fetch_one(pool)
    .await
    .expect("seed pengguna")
}

/// Masuk lewat `/v1/auth/masuk` dan kembalikan (token akses, refresh token).
pub async fn masuk(app: &Router, akun: &str) -> (String, String) {
    let r = kirim(
        app,
        "POST",
        "/v1/auth/masuk",
        &[],
        Some(serde_json::json!({"akun": akun, "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "masuk gagal: {}", r.json);
    (
        r.json["token"].as_str().unwrap().to_string(),
        r.json["refresh_token"].as_str().unwrap().to_string(),
    )
}

/// Header bearer.
pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// Sesi operator gudang: token shift + `X-Device-Key`.
pub struct SesiGudang {
    pub auth: String,
}

impl SesiGudang {
    /// Header untuk endpoint desktop gudang.
    pub fn h(&self) -> [(&str, &str); 2] {
        [
            ("authorization", self.auth.as_str()),
            ("x-device-key", KUNCI_GUDANG),
        ]
    }
}

/// Seed operator `OPR-SORTIR-1` lalu login shift lewat `/v1/gudang/masuk`.
pub async fn masuk_gudang(app: &Router, pool: &PgPool) -> SesiGudang {
    buat_pengguna(pool, "OPR-SORTIR-1", Peran::AdminGudang).await;
    let r = kirim(
        app,
        "POST",
        "/v1/gudang/masuk",
        &[("x-device-key", KUNCI_GUDANG)],
        Some(serde_json::json!({"operator": "OPR-SORTIR-1", "pin": PIN_UJI})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "shift gagal: {}", r.json);
    SesiGudang {
        auth: bearer(r.json["token"].as_str().unwrap()),
    }
}

/// Seed petani aktif.
pub async fn buat_petani(pool: &PgPool, kode: &str) {
    sqlx::query("INSERT INTO petani (kode, nama) VALUES ($1, $1)")
        .bind(kode)
        .execute(pool)
        .await
        .expect("seed petani");
}

/// Kirim badan biner dengan `content-type` bebas (multipart foto).
pub async fn kirim_bytes(
    app: &Router,
    uri: &str,
    header: &[(&str, &str)],
    content_type: &str,
    badan: Vec<u8>,
) -> Resp {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", content_type);
    for (k, v) in header {
        b = b.header(*k, *v);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(badan)).unwrap())
        .await
        .expect("respons");
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    Resp {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        headers,
    }
}

/// Badan multipart `uuid` + `berkas` (JPEG); mengembalikan (content-type, badan).
pub fn multipart_foto(uuid: Option<&str>, berkas: &[u8]) -> (String, Vec<u8>) {
    let batas = "BATAS-UJI-LUMBUNG";
    let mut badan = Vec::new();
    if let Some(u) = uuid {
        badan.extend_from_slice(
            format!("--{batas}\r\nContent-Disposition: form-data; name=\"uuid\"\r\n\r\n{u}\r\n")
                .as_bytes(),
        );
    }
    badan.extend_from_slice(
        format!(
            "--{batas}\r\nContent-Disposition: form-data; name=\"berkas\"; filename=\"f.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
        )
        .as_bytes(),
    );
    badan.extend_from_slice(berkas);
    badan.extend_from_slice(format!("\r\n--{batas}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={batas}"), badan)
}

/// JPEG sintetis utuh (SOI + isi + EOI) sebesar `ukuran` byte.
pub fn jpeg_uji(ukuran: usize) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0];
    v.resize(ukuran.saturating_sub(2).max(4), 0x11);
    v.extend_from_slice(&[0xFF, 0xD9]);
    v
}
