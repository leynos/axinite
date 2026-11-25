//! Domain-separated authenticated encryption for endpoint bindings.
//!
//! Endpoint bindings reuse the same AES-256-GCM construction as credentials
//! but never share a key domain with them. Their HKDF info string differs, and
//! every seal is authenticated against caller-supplied associated data that
//! names the binding's scope, kind, envelope version, and validation-policy
//! version. Copying a ciphertext into another row, another column, or another
//! user's scope therefore fails authentication instead of decrypting.
//!
//! The credential API in [`super`] is deliberately untouched, so existing
//! secret ciphertext keeps its format and key domain.

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use hkdf::Hkdf;
use secrecy::ExposeSecret;
use sha2::Sha256;

use super::{KEY_SIZE, NONCE_SIZE, SecretsCrypto, TAG_SIZE};
use crate::secrets::types::{DecryptedSecret, SecretError};

/// HKDF info string for the endpoint-binding key domain.
///
/// Must never equal the credential info string, so a key derived for one
/// domain cannot decrypt a ciphertext sealed in the other.
const ENDPOINT_BINDING_INFO: &[u8] = b"axinite-endpoint-binding-v1";

impl SecretsCrypto {
    /// Seal an endpoint binding under the endpoint-binding key domain.
    ///
    /// Returns `(encrypted_value, salt)` with the same envelope layout as a
    /// credential: `nonce || ciphertext || tag`, with the salt stored
    /// separately. `associated_data` is authenticated but not encrypted; the
    /// caller must supply exactly the same bytes to open the record.
    pub fn encrypt_endpoint_binding(
        &self,
        plaintext: &[u8],
        associated_data: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), SecretError> {
        let salt = Self::generate_salt();
        let derived_key = self.derive_key_for(ENDPOINT_BINDING_INFO, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(&derived_key).map_err(|_| {
            SecretError::EncryptionFailed("Failed to create endpoint cipher".to_string())
        })?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| {
                SecretError::EncryptionFailed("Endpoint binding encryption failed".to_string())
            })?;

        let mut encrypted = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        encrypted.extend_from_slice(&nonce);
        encrypted.extend_from_slice(&ciphertext);

        Ok((encrypted, salt))
    }

    /// Open an endpoint binding, requiring both the record's associated data
    /// and the endpoint-binding key domain.
    ///
    /// Any mismatch in key, salt, ciphertext, nonce, or associated data
    /// surfaces as [`SecretError::DecryptionFailed`] without describing the
    /// stored value.
    pub fn decrypt_endpoint_binding(
        &self,
        encrypted_value: &[u8],
        salt: &[u8],
        associated_data: &[u8],
    ) -> Result<DecryptedSecret, SecretError> {
        if encrypted_value.len() < NONCE_SIZE + TAG_SIZE {
            return Err(SecretError::DecryptionFailed(
                "Endpoint binding value too short".to_string(),
            ));
        }

        let derived_key = self.derive_key_for(ENDPOINT_BINDING_INFO, salt)?;
        let cipher = Aes256Gcm::new_from_slice(&derived_key).map_err(|_| {
            SecretError::DecryptionFailed("Failed to create endpoint cipher".to_string())
        })?;

        let (nonce_bytes, ciphertext) = encrypted_value.split_at(NONCE_SIZE);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: associated_data,
                },
            )
            .map_err(|_| {
                SecretError::DecryptionFailed("Endpoint binding decryption failed".to_string())
            })?;

        DecryptedSecret::from_bytes(plaintext)
    }

    /// Derive a key in an explicit HKDF domain.
    fn derive_key_for(&self, info: &[u8], salt: &[u8]) -> Result<[u8; KEY_SIZE], SecretError> {
        let hk = Hkdf::<Sha256>::new(Some(salt), self.master_key.expose_secret().as_bytes());

        let mut derived = [0u8; KEY_SIZE];
        hk.expand(info, &mut derived)
            .map_err(|_| SecretError::EncryptionFailed("HKDF expansion failed".to_string()))?;

        Ok(derived)
    }
}

#[cfg(test)]
mod tests {
    //! Tamper and domain-separation tests for endpoint-binding encryption.

    use secrecy::SecretString;

    use super::super::SecretsCrypto;
    use crate::testing::credentials::TEST_CRYPTO_KEY;

    const AAD: &[u8] = b"scope-a";

    fn crypto() -> SecretsCrypto {
        SecretsCrypto::new(SecretString::from(TEST_CRYPTO_KEY.to_string()))
            .expect("test key is long enough")
    }

    #[test]
    fn roundtrip_requires_the_same_associated_data() {
        let crypto = crypto();
        let (encrypted, salt) = crypto
            .encrypt_endpoint_binding(b"https://mail.example.com/", AAD)
            .expect("seal");

        let opened = crypto
            .decrypt_endpoint_binding(&encrypted, &salt, AAD)
            .expect("open");
        assert_eq!(opened.expose(), "https://mail.example.com/");
    }

    /// What a test deliberately damages before attempting to open a seal.
    enum Tamper {
        /// Open with a different associated data value.
        Aad(&'static [u8]),
        /// Flip one bit of the ciphertext.
        Ciphertext,
        /// Substitute an unrelated salt.
        Salt,
    }

    /// Seal one value, apply `tamper`, and require the open to fail.
    ///
    /// Sharing the seal-and-open shape keeps each test to the single fact it
    /// asserts, which is what the tamper variant names.
    fn assert_open_fails(tamper: Tamper) {
        let crypto = crypto();
        let (encrypted, salt) = crypto
            .encrypt_endpoint_binding(b"https://mail.example.com/", AAD)
            .expect("seal");

        let mut ciphertext = encrypted.clone();
        let mut key_salt = salt.clone();
        let open_aad = match tamper {
            Tamper::Aad(aad) => aad,
            Tamper::Ciphertext => {
                *ciphertext.last_mut().expect("non-empty envelope") ^= 0x01;
                AAD
            }
            Tamper::Salt => {
                key_salt = vec![0u8; salt.len()];
                AAD
            }
        };

        assert!(
            crypto
                .decrypt_endpoint_binding(&ciphertext, &key_salt, open_aad)
                .is_err(),
            "a tampered envelope or a mismatched scope must not open"
        );
    }

    #[test]
    fn different_associated_data_fails() {
        assert_open_fails(Tamper::Aad(b"scope-b"));
    }

    #[test]
    fn endpoint_ciphertext_is_not_a_credential_ciphertext() {
        let crypto = crypto();
        let (encrypted, salt) = crypto
            .encrypt_endpoint_binding(b"https://mail.example.com/", AAD)
            .expect("seal");

        assert!(crypto.decrypt(&encrypted, &salt).is_err());
    }

    #[test]
    fn credential_ciphertext_is_not_an_endpoint_ciphertext() {
        let crypto = crypto();
        let (encrypted, salt) = crypto.encrypt(b"api-token").expect("seal");

        assert!(
            crypto
                .decrypt_endpoint_binding(&encrypted, &salt, AAD)
                .is_err()
        );
    }

    #[test]
    fn tampered_ciphertext_fails() {
        assert_open_fails(Tamper::Ciphertext);
    }

    #[test]
    fn wrong_salt_fails() {
        assert_open_fails(Tamper::Salt);
    }

    #[test]
    fn wrong_master_key_fails() {
        let sealer = crypto();
        let other = SecretsCrypto::new(SecretString::from("b".repeat(32))).expect("test key");
        let (encrypted, salt) = sealer
            .encrypt_endpoint_binding(b"https://mail.example.com/", AAD)
            .expect("seal");

        assert!(
            other
                .decrypt_endpoint_binding(&encrypted, &salt, AAD)
                .is_err()
        );
    }

    #[test]
    fn truncated_envelope_fails_without_panicking() {
        let crypto = crypto();
        let salt = SecretsCrypto::generate_salt();

        assert!(
            crypto
                .decrypt_endpoint_binding(&[0u8; 27], &salt, AAD)
                .is_err()
        );
    }

    #[test]
    fn envelope_layout_matches_credentials() {
        let crypto = crypto();
        let plaintext = b"https://mail.example.com/";
        let (encrypted, salt) = crypto
            .encrypt_endpoint_binding(plaintext, AAD)
            .expect("seal");

        assert_eq!(encrypted.len(), 12 + plaintext.len() + 16);
        assert_eq!(salt.len(), 32);
    }
}
