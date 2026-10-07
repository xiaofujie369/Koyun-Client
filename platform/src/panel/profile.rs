use super::PanelError;
use serde_yaml_ng::Value;
use sha2::{Digest, Sha256};
use std::fmt;
use zeroize::Zeroizing;

pub const MAX_PROFILE_BYTES: usize = 8 * 1024 * 1024;

pub struct ProfileDocument {
    content: Zeroizing<Vec<u8>>,
    etag: String,
}

impl ProfileDocument {
    pub fn parse(content: Vec<u8>) -> Result<Self, PanelError> {
        let content = Zeroizing::new(content);
        if content.len() > MAX_PROFILE_BYTES {
            return Err(PanelError::ResponseTooLarge);
        }
        let parsed: Value =
            serde_yaml_ng::from_slice(&content).map_err(|_| PanelError::InvalidProfile)?;
        let map = parsed.as_mapping().ok_or(PanelError::InvalidProfile)?;
        let proxies = map.get(Value::String("proxies".into()));
        let providers = map.get(Value::String("proxy-providers".into()));
        let valid_proxies = proxies.is_some_and(|v| v.as_sequence().is_some_and(|s| !s.is_empty()));
        let valid_providers =
            providers.is_some_and(|v| v.as_mapping().is_some_and(|m| !m.is_empty()));
        if !valid_proxies && !valid_providers {
            return Err(PanelError::InvalidProfile);
        }
        let etag = format!("\"{:x}\"", Sha256::digest(&*content));
        Ok(Self { content, etag })
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }

    pub fn etag(&self) -> &str {
        &self.etag
    }
}

impl fmt::Debug for ProfileDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileDocument")
            .field("content", &"[REDACTED]")
            .finish()
    }
}
