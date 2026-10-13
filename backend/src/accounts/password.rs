use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};

use crate::accounts::error::AccountError;

const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 128;

pub fn hash_password(password: &str) -> Result<String, AccountError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| AccountError::Password {
            message: error.to_string(),
        })?;
    Ok(hash.to_string())
}

pub fn verify_password(password: &str, password_hash: &str) -> Result<bool, AccountError> {
    let parsed = PasswordHash::new(password_hash).map_err(|error| AccountError::Password {
        message: error.to_string(),
    })?;

    match Argon2::default().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(error) => Err(AccountError::Password {
            message: error.to_string(),
        }),
    }
}

pub fn validate_password(password: &str) -> Result<(), AccountError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AccountError::Validation {
            field: "password",
            message: format!("must be at least {MIN_PASSWORD_LEN} characters"),
        });
    }
    if password.chars().count() > MAX_PASSWORD_LEN {
        return Err(AccountError::Validation {
            field: "password",
            message: format!("must be at most {MAX_PASSWORD_LEN} characters"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{hash_password, validate_password, verify_password};

    #[test]
    fn hashes_and_verifies_passwords() {
        let hash = hash_password("correct horse battery").expect("hash");
        assert!(hash.starts_with("$argon2"));
        assert_ne!(hash, "correct horse battery");

        assert!(verify_password("correct horse battery", &hash).expect("verify"));
        assert!(!verify_password("wrong password", &hash).expect("verify"));
    }

    #[test]
    fn hashes_are_salted_per_call() {
        let first = hash_password("same-secret").expect("hash");
        let second = hash_password("same-secret").expect("hash");
        assert_ne!(first, second, "each hash must use a fresh salt");
    }

    #[test]
    fn enforces_password_policy() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("long-enough-password").is_ok());
    }
}
