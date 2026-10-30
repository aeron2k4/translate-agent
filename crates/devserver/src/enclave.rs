//! Mock enclave cho chạy local (không có TEE thật).
//!
//! Ở môi trường thật, client lấy khoá phiên qua attestation của enclave rồi mã
//! hoá tin bằng khoá đó, nên máy chủ ngoài enclave không thấy bản rõ. Ở đây khoá
//! là khoá dev cố định — KHÔNG an toàn, chỉ để chạy thử luồng mã hoá đầu-cuối.

use aes_gcm::aead::{Aead, KeyInit};
#[cfg(test)]
use aes_gcm::aead::{AeadCore, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Attestation {
    pub enclave: &'static str,
    pub measurement: &'static str,
    pub key_id: &'static str,
    pub warning: &'static str,
}

/// Tin đã mã hoá bằng khoá phiên (hex).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sealed {
    pub key_id: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnclaveError {
    UnknownKey,
    Malformed,
    Decrypt,
}

impl EnclaveError {
    pub fn code(self) -> &'static str {
        match self {
            Self::UnknownKey => "enclave_unknown_key",
            Self::Malformed => "enclave_malformed",
            Self::Decrypt => "enclave_decrypt_failed",
        }
    }
}

pub trait Enclave: Send + Sync {
    fn attestation(&self) -> Attestation;
    fn unseal(&self, sealed: &Sealed) -> Result<String, EnclaveError>;
}

pub struct MockEnclave {
    cipher: Aes256Gcm,
}

const DEV_KEY_ID: &str = "dev";

impl MockEnclave {
    pub fn new(dev_key: [u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new(&dev_key.into()),
        }
    }

    /// Mã hoá như client sẽ làm.
    #[cfg(test)]
    pub fn seal(&self, plaintext: &str) -> Option<Sealed> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self.cipher.encrypt(&nonce, plaintext.as_bytes()).ok()?;
        Some(Sealed {
            key_id: DEV_KEY_ID.into(),
            nonce: hex::encode(nonce),
            ciphertext: hex::encode(ct),
        })
    }
}

impl Enclave for MockEnclave {
    fn attestation(&self) -> Attestation {
        Attestation {
            enclave: "mock",
            measurement: "dev-only-not-attested",
            key_id: DEV_KEY_ID,
            warning: "Mock enclave cho chạy local; không dùng với dữ liệu thật.",
        }
    }

    fn unseal(&self, sealed: &Sealed) -> Result<String, EnclaveError> {
        if sealed.key_id != DEV_KEY_ID {
            return Err(EnclaveError::UnknownKey);
        }
        let nonce = hex::decode(&sealed.nonce).map_err(|_| EnclaveError::Malformed)?;
        if nonce.len() != 12 {
            return Err(EnclaveError::Malformed);
        }
        let ct = hex::decode(&sealed.ciphertext).map_err(|_| EnclaveError::Malformed)?;
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce), ct.as_slice())
            .map_err(|_| EnclaveError::Decrypt)?;
        String::from_utf8(plain).map_err(|_| EnclaveError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_unseal_round_trip() {
        let e = MockEnclave::new([7; 32]);
        let s = e.seal("cọc 5 củ").unwrap();
        assert_ne!(s.ciphertext, hex::encode("cọc 5 củ"));
        assert_eq!(e.unseal(&s).unwrap(), "cọc 5 củ");
        let mut bad = s.clone();
        bad.ciphertext.replace_range(0..2, "00");
        assert_eq!(e.unseal(&bad), Err(EnclaveError::Decrypt));
        bad.key_id = "other".into();
        assert_eq!(e.unseal(&bad), Err(EnclaveError::UnknownKey));
    }
}
