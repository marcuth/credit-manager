//! Criptografia dos segredos e dos backups.
//!
//! A chave vem da senha do usuário (Argon2id) e cifra com ChaCha20-Poly1305 (autenticado:
//! qualquer alteração no texto cifrado é detectada). Não existe recuperação de senha: sem ela,
//! o conteúdo cifrado não pode ser lido.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use chacha20poly1305::{
    ChaCha20Poly1305, Nonce,
    aead::{Aead, Key, KeyInit},
};

const NONCE_LEN: usize = 12;
pub const SALT_LEN: usize = 16;
/// Prefixo dos valores cifrados guardados como texto (tabela `settings`).
const TEXT_PREFIX: &str = "enc:v1:";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("senha incorreta ou dados corrompidos")]
    Decrypt,
    #[error("dados cifrados inválidos")]
    Malformed,
    #[error("falha ao derivar a chave: {0}")]
    Kdf(String),
    #[error("sem fonte de números aleatórios: {0}")]
    Random(String),
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| CryptoError::Random(e.to_string()))?;
    Ok(buf)
}

/// Chave de 256 bits já derivada da senha. Vive só na memória.
#[derive(Clone)]
pub struct Vault {
    key: [u8; 32],
}

impl Vault {
    /// Deriva a chave com Argon2id (19 MiB, 2 passes: confortável até em PC antigo).
    pub fn derive(password: &str, salt: &[u8]) -> Result<Self, CryptoError> {
        let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|e| CryptoError::Kdf(e.to_string()))?;
        let mut key = [0u8; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .map_err(|e| CryptoError::Kdf(e.to_string()))?;
        Ok(Self { key })
    }

    /// `nonce (12 bytes) || texto cifrado`.
    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce_bytes = random_bytes::<NONCE_LEN>()?;
        let cipher = ChaCha20Poly1305::new(&Key::<ChaCha20Poly1305>::from(self.key));
        let ciphertext = cipher
            .encrypt(&Nonce::from(nonce_bytes), plaintext)
            .map_err(|_| CryptoError::Decrypt)?;
        let mut out = nonce_bytes.to_vec();
        out.extend(ciphertext);
        Ok(out)
    }

    pub fn open(&self, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < NONCE_LEN + 16 {
            return Err(CryptoError::Malformed);
        }
        let (nonce, ciphertext) = blob.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CryptoError::Malformed)?;
        let cipher = ChaCha20Poly1305::new(&Key::<ChaCha20Poly1305>::from(self.key));
        cipher.decrypt(&Nonce::from(nonce), ciphertext).map_err(|_| CryptoError::Decrypt)
    }

    /// Cifra um texto para guardar em coluna de texto: `enc:v1:<base64>`.
    pub fn seal_text(&self, plaintext: &str) -> Result<String, CryptoError> {
        Ok(format!("{TEXT_PREFIX}{}", B64.encode(self.seal(plaintext.as_bytes())?)))
    }

    /// Abre um valor de [`Vault::seal_text`]. Texto sem o prefixo é devolvido como está
    /// (valor antigo, ainda não cifrado).
    pub fn open_text(&self, stored: &str) -> Result<String, CryptoError> {
        let Some(encoded) = stored.strip_prefix(TEXT_PREFIX) else {
            return Ok(stored.to_string());
        };
        let blob = B64.decode(encoded).map_err(|_| CryptoError::Malformed)?;
        String::from_utf8(self.open(&blob)?).map_err(|_| CryptoError::Malformed)
    }
}

pub fn is_sealed_text(value: &str) -> bool {
    value.starts_with(TEXT_PREFIX)
}

pub fn b64_encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn b64_decode(text: &str) -> Result<Vec<u8>, CryptoError> {
    B64.decode(text).map_err(|_| CryptoError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(pw: &str) -> Vault {
        Vault::derive(pw, &[9u8; SALT_LEN]).unwrap()
    }

    #[test]
    fn roundtrip() {
        let v = vault("segredo");
        let blob = v.seal(b"token-123").unwrap();
        assert_ne!(&blob[NONCE_LEN..], b"token-123");
        assert_eq!(v.open(&blob).unwrap(), b"token-123");
    }

    #[test]
    fn wrong_password_fails() {
        let blob = vault("certa").seal(b"x").unwrap();
        assert_eq!(vault("errada").open(&blob), Err(CryptoError::Decrypt));
    }

    #[test]
    fn tampering_is_detected() {
        let v = vault("s");
        let mut blob = v.seal(b"dados importantes").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 1;
        assert_eq!(v.open(&blob), Err(CryptoError::Decrypt));
        assert_eq!(v.open(&[1, 2, 3]), Err(CryptoError::Malformed));
    }

    #[test]
    fn nonce_is_unique_per_message() {
        let v = vault("s");
        assert_ne!(v.seal(b"a").unwrap(), v.seal(b"a").unwrap());
    }

    #[test]
    fn text_roundtrip_and_legacy_plaintext() {
        let v = vault("s");
        let sealed = v.seal_text("APP_USR-abc").unwrap();
        assert!(is_sealed_text(&sealed));
        assert_eq!(v.open_text(&sealed).unwrap(), "APP_USR-abc");
        assert_eq!(v.open_text("valor-antigo").unwrap(), "valor-antigo");
    }

    #[test]
    fn different_salt_gives_different_key() {
        let a = Vault::derive("s", &[1u8; SALT_LEN]).unwrap();
        let b = Vault::derive("s", &[2u8; SALT_LEN]).unwrap();
        assert!(b.open(&a.seal(b"x").unwrap()).is_err());
    }
}
