use botstral_owner_client::{ClientError, SecretStore};
use zeroize::Zeroizing;

pub struct NativeSecrets {
    entry: keyring::Entry,
}
impl NativeSecrets {
    pub fn new(slot: &str) -> Result<Self, ClientError> {
        if !cfg!(any(target_os = "macos", target_os = "linux")) {
            return Err(ClientError::Storage);
        }
        Ok(Self {
            entry: keyring::Entry::new("com.botstral.desktop.owner.v1", slot)
                .map_err(|_| ClientError::Storage)?,
        })
    }
}
impl SecretStore for NativeSecrets {
    fn read(&self) -> Result<Option<Zeroizing<Vec<u8>>>, ClientError> {
        match self.entry.get_secret() {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(ClientError::Storage),
        }
    }
    fn replace(&self, bytes: Zeroizing<Vec<u8>>) -> Result<(), ClientError> {
        self.entry
            .set_secret(&bytes)
            .map_err(|_| ClientError::Storage)
    }
    fn delete(&self) -> Result<(), ClientError> {
        match self.entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(ClientError::Storage),
        }
    }
}
