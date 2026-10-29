//! Ephemeral frames are deliberately absent from OwnerOperation and its replay journal.
use crate::{CredentialWriteTarget, RequestId};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const PREFIX: &[u8] = b"botstral-protected-owner-v1\0";
pub const MAX_PROTECTED_FRAME: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginValues {
    pub username: String,
    pub password: String,
    pub totp_seed: String,
}
impl Drop for LoginValues {
    fn drop(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
        self.totp_seed.zeroize();
    }
}
impl LoginValues {
    pub fn valid(&self) -> bool {
        !self.username.is_empty()
            && self.username.len() <= 512
            && !self.password.is_empty()
            && self.password.len() <= 4096
            && self.totp_seed.len() <= 128
            && [&self.username, &self.password, &self.totp_seed]
                .iter()
                .all(|s| !s.contains('\0'))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProtectedOwnerOperation {
    Write {
        target: CredentialWriteTarget,
        values: LoginValues,
    },
    Unlock {
        passphrase: Zeroizing<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedOwnerRequest {
    pub id: RequestId,
    pub operation: ProtectedOwnerOperation,
}
impl ProtectedOwnerRequest {
    pub fn encode(&self) -> Result<Zeroizing<Vec<u8>>, crate::ErrorCode> {
        let mut bytes = Zeroizing::new(PREFIX.to_vec());
        serde_json::to_writer(&mut *bytes, self).map_err(|_| crate::ErrorCode::InvalidMessage)?;
        if bytes.len() > MAX_PROTECTED_FRAME {
            return Err(crate::ErrorCode::InvalidMessage);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, crate::ErrorCode> {
        if bytes.len() > MAX_PROTECTED_FRAME || !bytes.starts_with(PREFIX) {
            return Err(crate::ErrorCode::InvalidMessage);
        }
        serde_json::from_slice(&bytes[PREFIX.len()..]).map_err(|_| crate::ErrorCode::InvalidMessage)
    }
}
