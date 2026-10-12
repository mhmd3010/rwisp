#![allow(deprecated)]

use std::fmt;
use std::sync::Arc;

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};

pub const ENCRYPTION_KEY_ENV: &str = "ROUTER_ENCRYPTION_KEY";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

#[derive(Debug)]
pub enum CryptoError {
    MissingKey { variable: &'static str },
    InvalidKeyFormat { reason: String },
    Encrypt { reason: String },
    Decrypt { reason: String },
    InvalidPlaintext,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey { variable } => {
                write!(f, "{variable} is not set")
            }
            Self::InvalidKeyFormat { reason } => {
                write!(f, "invalid encryption key format: {reason}")
            }
            Self::Encrypt { reason } => write!(f, "failed to encrypt credential: {reason}"),
            Self::Decrypt { reason } => write!(f, "failed to decrypt credential: {reason}"),
            Self::InvalidPlaintext => {
                write!(f, "decrypted credential is not valid UTF-8")
            }
        }
    }
}

impl std::error::Error for CryptoError {}

pub struct EncryptionKey {
    cipher: Arc<Aes256Gcm>,
}

impl EncryptionKey {
    pub fn from_env() -> Result<Self, CryptoError> {
        let raw = std::env::var(ENCRYPTION_KEY_ENV).map_err(|_| CryptoError::MissingKey {
            variable: ENCRYPTION_KEY_ENV,
        })?;
        Self::from_hex(&raw)
    }

    pub fn from_hex(raw: &str) -> Result<Self, CryptoError> {
        let trimmed = raw.trim();
        let bytes = decode_hex(trimmed)?;
        if bytes.len() != KEY_LEN {
            return Err(CryptoError::InvalidKeyFormat {
                reason: format!(
                    "expected {KEY_LEN} bytes ({} hex characters), got {} bytes",
                    KEY_LEN * 2,
                    bytes.len()
                ),
            });
        }

        let cipher =
            Aes256Gcm::new_from_slice(&bytes).map_err(|error| CryptoError::InvalidKeyFormat {
                reason: error.to_string(),
            })?;

        Ok(Self {
            cipher: Arc::new(cipher),
        })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<Vec<u8>, CryptoError> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|error| CryptoError::Encrypt {
                reason: error.to_string(),
            })?;

        let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        output.extend_from_slice(nonce.as_slice());
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<String, CryptoError> {
        if data.len() <= NONCE_LEN {
            return Err(CryptoError::Decrypt {
                reason: "ciphertext is too short".to_string(),
            });
        }

        let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext =
            self.cipher
                .decrypt(nonce, ciphertext)
                .map_err(|error| CryptoError::Decrypt {
                    reason: error.to_string(),
                })?;

        String::from_utf8(plaintext).map_err(|_| CryptoError::InvalidPlaintext)
    }
}

impl fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EncryptionKey(<redacted>)")
    }
}

fn decode_hex(input: &str) -> Result<Vec<u8>, CryptoError> {
    if input.is_empty() {
        return Err(CryptoError::InvalidKeyFormat {
            reason: "value is empty".to_string(),
        });
    }
    if !input.len().is_multiple_of(2) {
        return Err(CryptoError::InvalidKeyFormat {
            reason: "hex value must have an even number of characters".to_string(),
        });
    }

    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(input.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let high = hex_value(pair[0]).ok_or_else(|| CryptoError::InvalidKeyFormat {
            reason: "value contains non-hexadecimal characters".to_string(),
        })?;
        let low = hex_value(pair[1]).ok_or_else(|| CryptoError::InvalidKeyFormat {
            reason: "value contains non-hexadecimal characters".to_string(),
        })?;
        output.push((high << 4) | low);
    }

    Ok(output)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
