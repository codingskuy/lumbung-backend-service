//! Uji LMB-BE-006: fee per kg, invoice mingguan, pembayaran petani H+1 (denda otomatis).

mod common;

use axum::{http::StatusCode, Router};
use chrono::{Duration, NaiveDate};
use common::*;
use lumbung_backend_service::{
    auth::peran::Peran,
    invoice::{self, senin_dari},
    pembayaran::tandai_dibayar,
    waktu,
};
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

fn hari_ini() -> NaiveDate {
    waktu::hari_ini_wib()
}

fn senin(minggu: i64) -> NaiveDate {
    senin_dari(hari_ini()) + Duration::weeks(minggu)
}

impl Uji {
    async fn gudang(&self, uri: &str, badan: Value) -> Resp {
        kirim(&self.app, "POST", uri, &self.sesi.h(), Some(badan)).await
    }

    async fn hitung(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.pool).await.unwrap()
    }

    /// Lot tersortir untuk petani; mengembalikan kode lot.
    async fn lot(
        &self,
        petani: &str,
        komoditas: &str,
        kotor: i64,
        a: i64,
        b: i64,
        c: i64,
    ) -> String {
        let r = self
            .gudang("/v1/lot", json!({"uuid": Uuid::now_v7(), "kode_petani": petani, "komoditas": komoditas, "tanggal": "2026-10-06", "berat_kotor_g": kotor}))
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        let kode = r.json["kode"].as_str().unwrap().to_string();
        let r = self
            .gudang(&format!("/v1/lot/{kode}/sortir"), json!({"uuid": Uuid::now_v7(), "lot_id": kode, "grade_a_g": a, "grade_b_g": b, "grade_c_g": c}))
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        kode
    }

    /// Terbitkan papan (kunci layanan) untuk minggu ke-`minggu` relatif minggu ini.
    async fn papan(&self, minggu: i64) {
        let r = kirim(
            &self.app,
            "POST",
            "/v1/admin/papan-harga",
            &[("x-service-key", KUNCI_LAYANAN)],
            Some(json!({"uuid": Uuid::now_v7(), "berlaku_mulai": senin(minggu).to_string(), "referensi": [
                {"komoditas": "cabai", "harga": [35000, 36000, 35000]},
                {"komoditas": "bawang", "harga": [31200, 30000, 33000]},
                {"komoditas": "kangkung", "harga": [7000, 7500, 6800]},
            ]})),
        )
        .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    }

    /// Trip dengan 3 titik: PWN-1 dekat (170 kg), LPK-2 jauh (30 kg), titik tanpa pesanan (50 kg).
    async fn trip_dengan_fee(&self) -> String {
        let lot = self
            .lot("PTN-007", "cabai", 1_000_000, 1_000_000, 0, 0)
            .await;
        let isi: Vec<Value> = [
            ("KRJ-001", 170_000),
            ("KRJ-002", 30_000),
            ("KRJ-003", 50_000),
        ]
        .iter()
        .map(|(k, g)| json!({"kode": k, "lot_id": lot, "grade": "A", "berat_g": g}))
        .collect();
        let r = self
            .gudang(
                "/v1/keranjang/batch",
                json!({"uuid": Uuid::now_v7(), "keranjang": isi}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        let r = self
            .gudang(
                "/v1/trip",
                json!({"uuid": Uuid::now_v7(), "rute": "UTARA", "kendaraan": "L 1", "keranjang": ["KRJ-001", "KRJ-002", "KRJ-003"],
                       "titik": [
                           {"id": "PWN-1", "zona": "DEKAT", "pesanan": "PSO-20261010-001", "pembeli": "PAWON-01", "keranjang": ["KRJ-001"]},
                           {"id": "LPK-2", "zona": "JAUH", "pesanan": "PSO-20261010-002", "pembeli": "LAPAK-02", "keranjang": ["KRJ-002"]},
                           {"id": "TTK-3", "zona": "DEKAT", "keranjang": ["KRJ-003"]},
                       ]}),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
        r.json["kode"].as_str().unwrap().to_string()
    }

    async fn surat_jalan(&self, trip: &str) -> Resp {
        self.gudang(
            &format!("/v1/trip/{trip}/surat-jalan"),
            json!({"uuid": Uuid::now_v7()}),
        )
        .await
    }

    /// Surat jalan + fee dipindah ke minggu lalu agar invoice minggu itu dapat terbit.
    async fn fee_minggu_lalu(&self) -> String {
        let trip = self.trip_dengan_fee().await;
        assert_eq!(self.surat_jalan(&trip).await.status, StatusCode::CREATED);
        sqlx::query("UPDATE fee SET tanggal = $1")
            .bind(senin(-1) + Duration::days(2))
            .execute(&self.pool)
            .await
            .unwrap();
        invoice::format_minggu(senin(-1))
    }

    async fn admin(&self, akun: &str, peran: Peran) -> String {
        buat_pengguna(&self.pool, akun, peran).await;
        bearer(&masuk(&self.app, akun).await.0)
    }

    async fn daftar_invoice(&self, minggu: &str) -> Resp {
        kirim(
            &self.app,
            "GET",
            &format!("/v1/admin/invoice?minggu={minggu}"),
            &[("x-service-key", KUNCI_LAYANAN)],
            None,
        )
        .await
    }
}

// ---------- fee ----------

#[sqlx::test]
async fn surat_jalan_menghitung_fee_per_pesanan_dari_berat_titik(pool: PgPool) {
    let u = siapkan(pool).await;
    let trip = u.trip_dengan_fee().await;
    let r = u.surat_jalan(&trip).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.json);
    // 170.000 g dekat = Rp255.000 (contoh BR-301); 30.000 g jauh = Rp75.000.
    assert_eq!(
        r.json["rincian_fee"],
        json!([
            {"pesanan": "PSO-20261010-001", "berat_g": 170_000, "zona": "DEKAT", "tarif_per_kg": 1_500, "nominal": 255_000},
            {"pesanan": "PSO-20261010-002", "berat_g": 30_000, "zona": "JAUH", "tarif_per_kg": 2_500, "nominal": 75_000},
        ])
    );
    assert_eq!(
        r.json["fee_total"], 330_000,
        "titik tanpa pesanan tidak ditagih"
    );
    assert_eq!(u.hitung("SELECT count(*) FROM fee").await, 2);
    assert_eq!(
        u.hitung("SELECT count(*) FROM fee WHERE status_cetak = 'belum'")
            .await,
        2
    );
    // Kirim ulang surat jalan tidak menggandakan fee.
    let sj_uuid: Uuid = sqlx::query_scalar("SELECT uuid_klien FROM surat_jalan")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    let r = u
        .gudang(
            &format!("/v1/trip/{trip}/surat-jalan"),
            json!({"uuid": sj_uuid}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(u.hitung("SELECT count(*) FROM fee").await, 2);
}

#[sqlx::test]
async fn pesanan_dobel_dan_pembeli_kosong_ditolak_saat_susun_trip(pool: PgPool) {
    let u = siapkan(pool).await;
    let lot = u.lot("PTN-007", "cabai", 1_000_000, 1_000_000, 0, 0).await;
    u.gudang(
        "/v1/keranjang/batch",
        json!({"uuid": Uuid::now_v7(), "keranjang": [
        {"kode": "KRJ-001", "lot_id": lot, "grade": "A", "berat_g": 1000},
        {"kode": "KRJ-002", "lot_id": lot, "grade": "A", "berat_g": 1000}]}),
    )
    .await;
    let r = u.gudang("/v1/trip", json!({"uuid": Uuid::now_v7(), "rute": "KOTA", "kendaraan": "L 1", "keranjang": ["KRJ-001", "KRJ-002"], "titik": [
        {"id": "A", "zona": "DEKAT", "pesanan": "PSO-20261010-001", "keranjang": ["KRJ-001"]},
        {"id": "B", "zona": "DEKAT", "pesanan": "PSO-20261010-001", "keranjang": ["KRJ-002"]}]})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = u.gudang("/v1/trip", json!({"uuid": Uuid::now_v7(), "rute": "KOTA", "kendaraan": "L 1", "keranjang": ["KRJ-001"], "titik": [
        {"id": "A", "zona": "DEKAT", "pesanan": "PSO-20261010-001", "pembeli": "  "}]})).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
}

// ---------- invoice ----------

#[sqlx::test]
async fn invoice_terbit_per_pembeli_senin_jatuh_tempo_7_hari_dan_idempoten(pool: PgPool) {
    let u = siapkan(pool).await;
    let minggu = u.fee_minggu_lalu().await;

    let r = u.daftar_invoice(&minggu).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let terbit = senin(0); // Senin setelah minggu yang ditagih
    let kode = |n: u32| format!("INV-{}-{n:03}", terbit.format("%Y%m%d"));
    assert_eq!(
        r.json["data"],
        json!([
            {"id": kode(1), "pembeli": "LAPAK-02", "minggu": minggu, "nominal": 75_000, "status": "TERBIT",
             "jatuh_tempo": (terbit + Duration::days(7)).to_string(), "denda": 0},
            {"id": kode(2), "pembeli": "PAWON-01", "minggu": minggu, "nominal": 255_000, "status": "TERBIT",
             "jatuh_tempo": (terbit + Duration::days(7)).to_string(), "denda": 0},
        ])
    );
    assert_eq!(
        u.hitung("SELECT count(*) FROM fee WHERE invoice_id IS NOT NULL")
            .await,
        2
    );

    // Idempoten: baca ulang dan generasi tertunda tidak membuat invoice baru.
    let r2 = u.daftar_invoice(&minggu).await;
    assert_eq!(r2.json, r.json);
    assert_eq!(
        invoice::terbitkan_tertunda(&u.pool, hari_ini())
            .await
            .unwrap(),
        0
    );
    assert_eq!(u.hitung("SELECT count(*) FROM invoice").await, 2);
}

#[sqlx::test]
async fn minggu_berjalan_belum_ditagih_dan_parameter_divalidasi(pool: PgPool) {
    let u = siapkan(pool).await;
    let trip = u.trip_dengan_fee().await;
    u.surat_jalan(&trip).await; // fee bertanggal hari ini (minggu berjalan)
    let ini = invoice::format_minggu(senin(0));
    let r = u.daftar_invoice(&ini).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["data"], json!([]), "invoice terbit Senin berikutnya");
    assert_eq!(u.hitung("SELECT count(*) FROM invoice").await, 0);
    assert_eq!(
        invoice::terbitkan_tertunda(&u.pool, hari_ini())
            .await
            .unwrap(),
        0
    );

    for uri in [
        "/v1/admin/invoice",
        "/v1/admin/invoice?minggu=",
        "/v1/admin/invoice?minggu=2026-41",
        "/v1/admin/invoice?minggu=2026-W54",
        "/v1/admin/invoice?minggu=2026-W00",
        "/v1/admin/invoice?minggu=2027-W53",
        "/v1/admin/invoice?minggu=kemarin",
    ] {
        let r = kirim(
            &u.app,
            "GET",
            uri,
            &[("x-service-key", KUNCI_LAYANAN)],
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{uri}");
    }
    // Invoice terbit sendiri oleh tugas latar bila minggunya sudah lewat.
    let satu_minggu_lagi = hari_ini() + Duration::days(7);
    assert_eq!(
        invoice::terbitkan_tertunda(&u.pool, satu_minggu_lagi)
            .await
            .unwrap(),
        2
    );
}

#[sqlx::test]
async fn denda_invoice_1_persen_per_minggu_maks_4_persen_dan_status_lewat_jatuh_tempo(
    pool: PgPool,
) {
    let u = siapkan(pool).await;
    let minggu = u.fee_minggu_lalu().await;
    u.daftar_invoice(&minggu).await;
    let cari = |r: &Resp, pembeli: &str| -> Value {
        r.json["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["pembeli"] == pembeli)
            .unwrap()
            .clone()
    };

    // Lewat 8 hari: 2 minggu berjalan = 2 persen dari Rp255.000 = Rp5.100.
    sqlx::query("UPDATE invoice SET terbit = $1, jatuh_tempo = $2")
        .bind(hari_ini() - Duration::days(15))
        .bind(hari_ini() - Duration::days(8))
        .execute(&u.pool)
        .await
        .unwrap();
    let r = u.daftar_invoice(&minggu).await;
    let pawon = cari(&r, "PAWON-01");
    assert_eq!(pawon["status"], "LEWAT_JATUH_TEMPO");
    assert_eq!(pawon["denda"], 5_100);
    assert_eq!(pawon["nominal"], 255_000, "nominal tidak memuat denda");

    // Sangat lama: dibatasi 4 persen = Rp10.200.
    sqlx::query("UPDATE invoice SET terbit = $1, jatuh_tempo = $2")
        .bind(hari_ini() - Duration::days(97))
        .bind(hari_ini() - Duration::days(90))
        .execute(&u.pool)
        .await
        .unwrap();
    let r = u.daftar_invoice(&minggu).await;
    assert_eq!(cari(&r, "PAWON-01")["denda"], 10_200);
    assert_eq!(cari(&r, "LAPAK-02")["denda"], 3_000);
}

#[sqlx::test]
async fn lunas_beraudit_membekukan_denda_dan_ketat_peran(pool: PgPool) {
    let u = siapkan(pool).await;
    let minggu = u.fee_minggu_lalu().await;
    let r = u.daftar_invoice(&minggu).await;
    let id = r.json["data"][1]["id"].as_str().unwrap().to_string(); // PAWON-01
    sqlx::query("UPDATE invoice SET terbit = $1, jatuh_tempo = $2")
        .bind(hari_ini() - Duration::days(15))
        .bind(hari_ini() - Duration::days(8))
        .execute(&u.pool)
        .await
        .unwrap();
    let uri = format!("/v1/admin/invoice/{id}/lunas");

    // Peran salah dan tanpa kredensial ditolak sebelum menyentuh data.
    let sopir = u.admin("sopir-02", Peran::Sopir).await;
    let koord = u.admin("koord-01", Peran::KoordinatorTani).await;
    let petani = u.admin("PTN-007", Peran::Petani).await;
    for (nama, header, harap) in [
        ("tanpa kredensial", vec![], StatusCode::UNAUTHORIZED),
        (
            "sopir",
            vec![("authorization", sopir.as_str())],
            StatusCode::FORBIDDEN,
        ),
        (
            "koordinator tani",
            vec![("authorization", koord.as_str())],
            StatusCode::FORBIDDEN,
        ),
        (
            "petani",
            vec![("authorization", petani.as_str())],
            StatusCode::FORBIDDEN,
        ),
        (
            "device key",
            vec![("x-device-key", KUNCI_GUDANG)],
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let r = kirim(&u.app, "POST", &uri, &header, None).await;
        assert_eq!(r.status, harap, "{nama}");
    }
    assert_eq!(
        u.hitung("SELECT count(*) FROM invoice WHERE dilunasi_at IS NOT NULL")
            .await,
        0
    );

    // Admin keuangan melunasi: audit_id AUD-YYYYMMDD-NNN dan denda 2 persen dibekukan.
    let keu = u.admin("keu-01", Peran::AdminKeuangan).await;
    let r = kirim(&u.app, "POST", &uri, &[("authorization", &keu)], None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    let audit_id = r.json["audit_id"].as_str().unwrap();
    assert!(
        audit_id.len() == 16 && audit_id.starts_with("AUD-"),
        "{audit_id}"
    );
    let aktor: String = sqlx::query_scalar("SELECT aktor FROM audit_log WHERE audit_id = $1")
        .bind(audit_id)
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(aktor, "keu-01");

    // Tahun depan pun denda tetap beku; status LUNAS.
    sqlx::query("UPDATE invoice SET jatuh_tempo = jatuh_tempo - 300, terbit = terbit - 300")
        .execute(&u.pool)
        .await
        .unwrap();
    let r = u.daftar_invoice(&minggu).await;
    let pawon = r.json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!(pawon["status"], "LUNAS");
    assert_eq!(pawon["denda"], 5_100);

    // Sekali saja: lunas ulang 409; invoice tak ada 404; layanan dan admin gudang boleh.
    let r = kirim(&u.app, "POST", &uri, &[("authorization", &keu)], None).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let r = kirim(
        &u.app,
        "POST",
        "/v1/admin/invoice/INV-20000101-001/lunas",
        &[("authorization", &keu)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let lain = r.json.clone();
    assert_eq!(lain["error"], "tidak ditemukan");
    let id2 = u.daftar_invoice(&minggu).await.json["data"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = kirim(
        &u.app,
        "POST",
        &format!("/v1/admin/invoice/{id2}/lunas"),
        &[("x-service-key", KUNCI_LAYANAN)],
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(
        u.hitung("SELECT count(*) FROM audit_log WHERE aksi = 'invoice_lunas'")
            .await,
        2
    );
    assert_eq!(
        u.hitung("SELECT count(*) FROM invoice WHERE dilunasi_at IS NOT NULL")
            .await,
        2
    );
}

#[sqlx::test]
async fn daftar_invoice_hanya_admin_keuangan_gudang_atau_layanan(pool: PgPool) {
    let u = siapkan(pool).await;
    let sopir = u.admin("sopir-02", Peran::Sopir).await;
    let koord = u.admin("koord-01", Peran::KoordinatorTani).await;
    let keu = u.admin("keu-01", Peran::AdminKeuangan).await;
    let uri = "/v1/admin/invoice?minggu=2026-W41";
    for (nama, header, harap) in [
        ("tanpa kredensial", vec![], StatusCode::UNAUTHORIZED),
        (
            "sopir",
            vec![("authorization", sopir.as_str())],
            StatusCode::FORBIDDEN,
        ),
        (
            "koordinator",
            vec![("authorization", koord.as_str())],
            StatusCode::FORBIDDEN,
        ),
        (
            "keuangan",
            vec![("authorization", keu.as_str())],
            StatusCode::OK,
        ),
        (
            "gudang",
            vec![("authorization", u.sesi.auth.as_str())],
            StatusCode::OK,
        ),
        (
            "layanan",
            vec![("x-service-key", KUNCI_LAYANAN)],
            StatusCode::OK,
        ),
        (
            "layanan salah",
            vec![("x-service-key", "salah")],
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let r = kirim(&u.app, "GET", uri, &header, None).await;
        assert_eq!(r.status, harap, "{nama}");
    }
}

// ---------- pembayaran petani ----------

async fn masuk_petani(u: &Uji, kode: &str) -> String {
    bearer(&masuk(&u.app, kode).await.0)
}

async fn bayar_saya(u: &Uji, token: &str) -> Resp {
    kirim(
        &u.app,
        "GET",
        "/v1/pembayaran-saya",
        &[("authorization", token)],
        None,
    )
    .await
}

#[sqlx::test]
async fn pembayaran_h_plus_1_dari_berat_bersih_per_grade_kali_harga_papan(pool: PgPool) {
    let u = siapkan(pool).await;
    buat_pengguna(&u.pool, "PTN-007", Peran::Petani).await;
    let token = masuk_petani(&u, "PTN-007").await;
    u.papan(0).await;
    // L-8812: A 100.000 g x 31.500 + B 14.000 g x 26.500 = Rp3.521.000; C tidak dibayar.
    let lot = u
        .lot("PTN-007", "cabai", 120_000, 100_000, 14_000, 6_000)
        .await;

    let r = bayar_saya(&u, &token).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(
        r.json["data"],
        json!([{"lot_id": lot, "nominal": 3_521_000, "status": "MENUNGGU",
                "jatuh_tempo": (hari_ini() + Duration::days(1)).to_string(), "denda": 0}])
    );

    // Komoditas lain memakai harganya sendiri: bawang A 6.000 x 28.000 + B 3.500 x 23.500.
    let bawang = u.lot("PTN-007", "bawang", 10_000, 6_000, 3_500, 500).await;
    let r = bayar_saya(&u, &token).await;
    let baris = r.json["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["lot_id"] == bawang.as_str())
        .unwrap()
        .clone();
    assert_eq!(baris["nominal"], 168_000 + 82_250);
}

#[sqlx::test]
async fn pembayaran_tanpa_papan_belum_tampil_dan_muncul_setelah_papan_terbit(pool: PgPool) {
    let u = siapkan(pool).await;
    buat_pengguna(&u.pool, "PTN-007", Peran::Petani).await;
    let token = masuk_petani(&u, "PTN-007").await;
    u.lot("PTN-007", "cabai", 120_000, 100_000, 14_000, 6_000)
        .await;
    assert_eq!(
        u.hitung("SELECT count(*) FROM pembayaran_petani").await,
        1,
        "kewajiban tercatat saat sortir"
    );
    assert_eq!(
        bayar_saya(&u, &token).await.json["data"],
        json!([]),
        "nominal belum bisa dihitung"
    );
    u.papan(0).await;
    assert_eq!(
        bayar_saya(&u, &token).await.json["data"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test]
async fn denda_pembayaran_setengah_persen_per_hari_maks_lima_persen(pool: PgPool) {
    let u = siapkan(pool).await;
    buat_pengguna(&u.pool, "PTN-007", Peran::Petani).await;
    let token = masuk_petani(&u, "PTN-007").await;
    for m in [-4, -3, -2, -1, 0] {
        u.papan(m).await;
    }
    u.lot("PTN-007", "cabai", 120_000, 100_000, 14_000, 6_000)
        .await;
    let geser = |hari: i64| {
        let pool = u.pool.clone();
        async move {
            // jatuh tempo = tanggal + 1; telat `hari` hari berarti tanggal = hari_ini - hari - 1.
            sqlx::query("UPDATE pembayaran_petani SET tanggal = $1, jatuh_tempo = $2")
                .bind(hari_ini() - Duration::days(hari + 1))
                .bind(hari_ini() - Duration::days(hari))
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    let baris = |r: Resp| r.json["data"][0].clone();

    geser(0).await; // jatuh tempo hari ini: belum terlambat
    let b = baris(bayar_saya(&u, &token).await);
    assert_eq!(
        (b["status"].as_str(), b["denda"].as_i64()),
        (Some("MENUNGGU"), Some(0))
    );
    geser(1).await;
    let b = baris(bayar_saya(&u, &token).await);
    assert_eq!(
        (b["status"].as_str(), b["denda"].as_i64()),
        (Some("TERLAMBAT"), Some(17_605)),
        "0,5 persen"
    );
    geser(3).await;
    assert_eq!(
        baris(bayar_saya(&u, &token).await)["denda"],
        52_815,
        "1,5 persen"
    );
    geser(10).await;
    assert_eq!(
        baris(bayar_saya(&u, &token).await)["denda"],
        176_050,
        "5 persen"
    );
    geser(25).await;
    assert_eq!(
        baris(bayar_saya(&u, &token).await)["denda"],
        176_050,
        "dibatasi 5 persen"
    );
}

#[sqlx::test]
async fn tandai_dibayar_membekukan_nominal_dan_denda(pool: PgPool) {
    let u = siapkan(pool).await;
    buat_pengguna(&u.pool, "PTN-007", Peran::Petani).await;
    let token = masuk_petani(&u, "PTN-007").await;
    for m in [-1, 0] {
        u.papan(m).await;
    }
    let lot = u
        .lot("PTN-007", "cabai", 120_000, 100_000, 14_000, 6_000)
        .await;
    sqlx::query("UPDATE pembayaran_petani SET tanggal = $1, jatuh_tempo = $2")
        .bind(hari_ini() - Duration::days(4))
        .bind(hari_ini() - Duration::days(3))
        .execute(&u.pool)
        .await
        .unwrap();

    let dibayar = tandai_dibayar(&u.pool, &lot, hari_ini()).await.unwrap();
    assert_eq!(
        (dibayar.status, dibayar.nominal, dibayar.denda),
        ("DIBAYAR", 3_521_000, 52_815)
    );
    // Beku: berapa pun waktu berlalu, nominal dan denda tidak berubah; status DIBAYAR.
    sqlx::query(
        "UPDATE pembayaran_petani SET jatuh_tempo = jatuh_tempo - 30, tanggal = tanggal - 30",
    )
    .execute(&u.pool)
    .await
    .unwrap();
    let r = bayar_saya(&u, &token).await;
    assert_eq!(r.json["data"][0]["status"], "DIBAYAR");
    assert_eq!(r.json["data"][0]["nominal"], 3_521_000);
    assert_eq!(r.json["data"][0]["denda"], 52_815);
    // Dua kali tidak boleh; lot tak ada 404.
    assert!(tandai_dibayar(&u.pool, &lot, hari_ini()).await.is_err());
    assert!(tandai_dibayar(&u.pool, "L-9999", hari_ini()).await.is_err());
}

#[sqlx::test]
async fn pembayaran_saya_difilter_server_per_petani_dan_hanya_peran_petani(pool: PgPool) {
    let u = siapkan(pool).await;
    buat_petani(&u.pool, "PTN-008").await;
    buat_pengguna(&u.pool, "PTN-007", Peran::Petani).await;
    buat_pengguna(&u.pool, "PTN-008", Peran::Petani).await;
    u.papan(0).await;
    let lot7 = u
        .lot("PTN-007", "cabai", 120_000, 100_000, 14_000, 6_000)
        .await;
    let lot8 = u.lot("PTN-008", "kangkung", 10_000, 9_000, 1_000, 0).await;
    let t7 = masuk_petani(&u, "PTN-007").await;
    let t8 = masuk_petani(&u, "PTN-008").await;

    let r7 = bayar_saya(&u, &t7).await;
    let r8 = bayar_saya(&u, &t8).await;
    assert_eq!(r7.json["data"].as_array().unwrap().len(), 1);
    assert_eq!(r7.json["data"][0]["lot_id"], lot7.as_str());
    assert_eq!(r8.json["data"].as_array().unwrap().len(), 1);
    assert_eq!(r8.json["data"][0]["lot_id"], lot8.as_str());
    // Kangkung: A 9.000 x 6.000 + B 1.000 x 5.000 = 54.000 + 5.000.
    assert_eq!(r8.json["data"][0]["nominal"], 59_000);

    // Petani tidak bisa meminta data orang lain lewat parameter apa pun.
    let r = kirim(
        &u.app,
        "GET",
        "/v1/pembayaran-saya?petani=PTN-007&kode_petani=PTN-007",
        &[("authorization", &t8)],
        None,
    )
    .await;
    assert_eq!(r.json, r8.json);

    // Peran lain dan tanpa token ditolak.
    let sopir = u.admin("sopir-02", Peran::Sopir).await;
    for (nama, header, harap) in [
        ("tanpa token", vec![], StatusCode::UNAUTHORIZED),
        (
            "sopir",
            vec![("authorization", sopir.as_str())],
            StatusCode::FORBIDDEN,
        ),
        (
            "admin gudang",
            vec![
                ("authorization", u.sesi.auth.as_str()),
                ("x-device-key", KUNCI_GUDANG),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            "kunci layanan",
            vec![("x-service-key", KUNCI_LAYANAN)],
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let r = kirim(&u.app, "GET", "/v1/pembayaran-saya", &header, None).await;
        assert_eq!(r.status, harap, "{nama}");
    }
}

#[sqlx::test]
async fn sortir_lewat_sinkron_juga_mencatat_pembayaran(pool: PgPool) {
    let u = siapkan(pool).await;
    let r = u
        .gudang(
            "/v1/sinkron/batch",
            json!({"rekaman": [
                {"uuid": Uuid::now_v7(), "jenis": "lot", "nomor_sementara": "TMP-aaaaaaaa",
                 "payload": {"kode_petani": "PTN-007", "komoditas": "cabai", "tanggal": "2026-10-06", "berat_kotor_g": 120_000}},
                {"uuid": Uuid::now_v7(), "jenis": "sortir",
                 "payload": {"lot_id": "TMP-aaaaaaaa", "grade_a_g": 100_000, "grade_b_g": 14_000, "grade_c_g": 6_000}},
            ]}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json);
    assert_eq!(u.hitung("SELECT count(*) FROM pembayaran_petani").await, 1);
    let (a, b): (i64, i64) = sqlx::query_as("SELECT berat_a_g, berat_b_g FROM pembayaran_petani")
        .fetch_one(&u.pool)
        .await
        .unwrap();
    assert_eq!(
        (a, b),
        (100_000, 14_000),
        "berat bersih Grade A dan B; C tidak dibayar"
    );
}
