//! Autentikasi dan otorisasi Lumbung (`docs/auth.md`).
//!
//! - [`jwt`]: JWT akses ed25519 (audiens `lumbung`) dan token shift gudang.
//! - [`pin`]: hash argon2 untuk PIN 6 digit.
//! - [`kunci`]: API key perangkat gudang (rotasi 90 hari, tenggang 7 hari).
//! - [`principal`]: ekstraktor Axum (bearer, `X-Device-Key`, `X-Service-Key`).
//! - [`handlers`]: endpoint `/auth/*` dan `/gudang/masuk|keluar`.

pub mod handlers;
pub mod jwt;
pub mod kunci;
pub mod peran;
pub mod pin;
pub mod principal;

/// Bandingkan dua string dalam waktu konstan (untuk kunci API).
pub fn sama_waktu_konstan(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    // Panjang boleh bocor; isi tidak.
    let mut selisih = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        selisih |= x ^ y;
    }
    selisih == 0
}

#[cfg(test)]
mod tests {
    use super::sama_waktu_konstan;

    #[test]
    fn perbandingan_konstan() {
        assert!(sama_waktu_konstan("abc", "abc"));
        assert!(!sama_waktu_konstan("abc", "abd"));
        assert!(!sama_waktu_konstan("abc", "abcd"));
        assert!(!sama_waktu_konstan("", "a"));
        assert!(sama_waktu_konstan("", ""));
    }
}
