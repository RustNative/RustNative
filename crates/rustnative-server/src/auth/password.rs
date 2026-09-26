//! Password hashing with Argon2id (the OWASP recommendation), in the PHC
//! string format, so parameters can be raised later without breaking the
//! hashes already stored.
//!
//! ```
//! use rustnative_server::auth::password::{hash, verify};
//!
//! let stored = hash("correct horse battery staple").unwrap();
//! assert!(stored.starts_with("$argon2id$"));
//! assert!(verify("correct horse battery staple", &stored));
//! assert!(!verify("Tr0ub4dor&3", &stored));
//! ```

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

/// Hashes `password` with a fresh random salt.
///
/// # Errors
///
/// The system has no randomness, or the password is unusable (for example
/// longer than Argon2 accepts).
pub fn hash(password: &str) -> Result<String, String> {
    let mut salt = [0u8; 16];
    getrandom::getrandom(&mut salt).map_err(|error| error.to_string())?;
    let salt = SaltString::encode_b64(&salt).map_err(|error| error.to_string())?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| error.to_string())
}

/// Whether `password` matches `stored` (constant-time).
#[must_use]
pub fn verify(password: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
}
