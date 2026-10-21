//! PIN 6 digit: validasi format dan hash argon2 (`argon2` `0.5.3`).

use std::sync::OnceLock;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// PIN sah bila tepat 6 digit ASCII.
pub fn format_valid(pin: &str) -> bool {
    pin.len() == 6 && pin.bytes().all(|b| b.is_ascii_digit())
}

/// Hash PIN dengan argon2id dan salt acak.
pub fn hash(pin: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hasil = Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("gagal hash PIN: {e}"))?;
    Ok(hasil.to_string())
}

/// Periksa PIN terhadap hash tersimpan; `false` bila hash rusak.
pub fn verifikasi(pin: &str, hash_tersimpan: &str) -> bool {
    match PasswordHash::new(hash_tersimpan) {
        Ok(h) => Argon2::default()
            .verify_password(pin.as_bytes(), &h)
            .is_ok(),
        Err(_) => false,
    }
}

/// Verifikasi palsu agar akun yang tidak ada memakan waktu serupa (anti enumerasi akun).
pub fn verifikasi_palsu(pin: &str) {
    static PALSU: OnceLock<String> = OnceLock::new();
    let h = PALSU.get_or_init(|| hash("000000").unwrap_or_default());
    let _ = verifikasi(pin, h);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pin() {
        assert!(format_valid("123456"));
        assert!(!format_valid("12345"));
        assert!(!format_valid("1234567"));
        assert!(!format_valid("12a456"));
        assert!(!format_valid("１２３４５６")); // digit lebar penuh ditolak
    }

    #[test]
    fn hash_dan_verifikasi() {
        let h = hash("123456").unwrap();
        assert_ne!(h, "123456");
        assert!(verifikasi("123456", &h));
        assert!(!verifikasi("654321", &h));
        assert!(!verifikasi("123456", "bukan-hash"));
    }
}
