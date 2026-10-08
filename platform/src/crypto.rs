use crate::{panel::Secret, scope::TenantId};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng, Payload, rand_core::RngCore},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub struct MasterKey(Zeroizing<[u8; 32]>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CryptoError {
    InvalidKey,
    InvalidCiphertext,
    InvalidToken,
    EntropyUnavailable,
}

impl MasterKey {
    pub fn from_hex(value: &str) -> Result<Self, CryptoError> {
        Ok(Self(decode_key(value)?))
    }

    pub fn encrypt(&self, plaintext: &[u8], context: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&*self.0).map_err(|_| CryptoError::InvalidKey)?;
        let mut nonce_bytes = [0u8; 12];
        OsRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|_| CryptoError::EntropyUnavailable)?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: context,
                },
            )
            .map_err(|_| CryptoError::InvalidCiphertext)?;
        let mut result = Vec::with_capacity(1 + 12 + ciphertext.len());
        result.push(1);
        result.extend_from_slice(nonce);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    pub fn decrypt(
        &self,
        ciphertext: &[u8],
        context: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        if ciphertext.len() < 29 || ciphertext[0] != 1 {
            return Err(CryptoError::InvalidCiphertext);
        }
        let cipher = Aes256Gcm::new_from_slice(&*self.0).map_err(|_| CryptoError::InvalidKey)?;
        cipher
            .decrypt(
                Nonce::from_slice(&ciphertext[1..13]),
                Payload {
                    msg: &ciphertext[13..],
                    aad: context,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| CryptoError::InvalidCiphertext)
    }
}

pub(crate) fn decode_key(value: &str) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    if value.len() != 64 || !value.is_ascii() {
        return Err(CryptoError::InvalidKey);
    }
    let mut bytes = Zeroizing::new([0u8; 32]);
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let byte = std::str::from_utf8(pair).map_err(|_| CryptoError::InvalidKey)?;
        bytes[index] = u8::from_str_radix(byte, 16).map_err(|_| CryptoError::InvalidKey)?;
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Access,
    Refresh,
    Realtime,
}

impl TokenKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Access => "a1",
            Self::Refresh => "r1",
            Self::Realtime => "w1",
        }
    }
}

pub struct GeneratedToken {
    pub plaintext: Secret,
    pub hash: [u8; 32],
}

pub fn generate_token(tenant: &TenantId, kind: TokenKind) -> Result<GeneratedToken, CryptoError> {
    let mut random = Zeroizing::new([0u8; 32]);
    OsRng
        .try_fill_bytes(&mut *random)
        .map_err(|_| CryptoError::EntropyUnavailable)?;
    let token = format!(
        "{}.{}.{}",
        kind.prefix(),
        tenant.as_str(),
        URL_SAFE_NO_PAD.encode(random.as_slice())
    );
    let hash = Sha256::digest(token.as_bytes()).into();
    Ok(GeneratedToken {
        plaintext: Secret::new(token),
        hash,
    })
}

// The parsed tenant selects a lookup scope; only a stored hash match authenticates it.
pub fn inspect_token(token: &str, kind: TokenKind) -> Result<(TenantId, [u8; 32]), CryptoError> {
    let mut parts = token.split('.');
    if parts.next() != Some(kind.prefix()) {
        return Err(CryptoError::InvalidToken);
    }
    let tenant = TenantId::parse(parts.next().ok_or(CryptoError::InvalidToken)?)
        .map_err(|_| CryptoError::InvalidToken)?;
    let random = parts.next().ok_or(CryptoError::InvalidToken)?;
    if parts.next().is_some() || random.len() != 43 {
        return Err(CryptoError::InvalidToken);
    }
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(random)
            .map_err(|_| CryptoError::InvalidToken)?,
    );
    if decoded.len() != 32 {
        return Err(CryptoError::InvalidToken);
    }
    Ok((tenant, Sha256::digest(token.as_bytes()).into()))
}
