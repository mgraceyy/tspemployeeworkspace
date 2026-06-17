use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use password_hash::rand_core::OsRng;

use crate::error::{AppError, AppResult};

pub fn hash_pin(pin: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?
        .to_string();
    Ok(hash)
}

pub fn verify_pin(pin: &str, pin_hash: &str) -> AppResult<bool> {
    let parsed =
        PasswordHash::new(pin_hash).map_err(|e| AppError::Internal(anyhow::anyhow!("{e}")))?;
    Ok(Argon2::default()
        .verify_password(pin.as_bytes(), &parsed)
        .is_ok())
}
