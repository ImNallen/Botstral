use super::{LoginPassword, OperationError, ProviderSecret, VaultPassphrase};
use age::secrecy::ExposeSecret;
use argon2::{
    Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString,
};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use zeroize::Zeroizing;

const PROVIDER_PREFIX: &[u8] = b"botstral-provider-v1\0mistral\0";

pub(super) fn random_token() -> String {
    use base64::Engine;
    let mut bytes = Zeroizing::new([0; 32]);
    OsRng.fill_bytes(bytes.as_mut());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes.as_slice())
}

pub(super) fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn argon2() -> Argon2<'static> {
    Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        Params::new(65536, 3, 1, Some(32)).expect("valid Argon2 parameters"),
    )
}

pub(super) fn hash_password(password: &LoginPassword) -> Result<String, OperationError> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()
        .hash_password(password.0.expose_secret().as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| OperationError::Internal)
}

pub(super) fn verify_password(password: &LoginPassword, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|hash| {
        argon2()
            .verify_password(password.0.expose_secret().as_bytes(), &hash)
            .is_ok()
    })
}

fn encrypt(encryptor: age::Encryptor, plaintext: &[u8]) -> Result<Vec<u8>, OperationError> {
    let mut ciphertext = Vec::new();
    let mut writer = encryptor
        .wrap_output(&mut ciphertext)
        .map_err(|_| OperationError::Internal)?;
    writer
        .write_all(plaintext)
        .map_err(|_| OperationError::Internal)?;
    writer.finish().map_err(|_| OperationError::Internal)?;
    Ok(ciphertext)
}

pub(super) fn new_identity(passphrase: VaultPassphrase) -> Result<Vec<u8>, OperationError> {
    let identity = age::x25519::Identity::generate().to_string();
    encrypt(
        age::Encryptor::with_user_passphrase(passphrase.0),
        identity.expose_secret().as_bytes(),
    )
}

pub(super) fn unlock(
    ciphertext: &[u8],
    passphrase: VaultPassphrase,
) -> Result<age::x25519::Identity, OperationError> {
    let identity = age::scrypt::Identity::new(passphrase.0);
    let mut plaintext = Zeroizing::new(Vec::new());
    age::Decryptor::new(ciphertext)
        .map_err(|_| OperationError::UnlockFailed)?
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| OperationError::UnlockFailed)?
        .take(1024)
        .read_to_end(&mut plaintext)
        .map_err(|_| OperationError::UnlockFailed)?;
    std::str::from_utf8(&plaintext)
        .map_err(|_| OperationError::UnlockFailed)?
        .parse()
        .map_err(|_| OperationError::UnlockFailed)
}

pub(super) fn encrypt_provider(
    identity: &age::x25519::Identity,
    secret: ProviderSecret,
) -> Result<Vec<u8>, OperationError> {
    let recipient = identity.to_public();
    let mut payload = Zeroizing::new(Vec::from(PROVIDER_PREFIX));
    payload.extend_from_slice(secret.0.expose_secret().as_bytes());
    encrypt(
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|_| OperationError::Internal)?,
        &payload,
    )
}

pub(super) fn decrypt_provider(
    identity: &age::x25519::Identity,
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, OperationError> {
    let mut plaintext = Zeroizing::new(Vec::new());
    age::Decryptor::new(ciphertext)
        .map_err(|_| OperationError::Internal)?
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|_| OperationError::Internal)?
        .take((PROVIDER_PREFIX.len() + 8192 + 1) as u64)
        .read_to_end(&mut plaintext)
        .map_err(|_| OperationError::Internal)?;
    let prefix = PROVIDER_PREFIX;
    if !plaintext.starts_with(prefix)
        || plaintext.len() > prefix.len() + 8192
        || plaintext.len() == prefix.len()
    {
        return Err(OperationError::Internal);
    }
    Ok(Zeroizing::new(plaintext[prefix.len()..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_round_trip_and_authenticated_tampering() {
        let passphrase = || VaultPassphrase::new("synthetic vault passphrase only".into()).unwrap();
        let mut encrypted = new_identity(passphrase()).unwrap();
        assert!(
            unlock(
                &encrypted,
                VaultPassphrase::new("synthetic wrong passphrase".into()).unwrap()
            )
            .is_err()
        );
        let identity = unlock(&encrypted, passphrase()).unwrap();
        let key = b"synthetic-provider-key-only";
        let mut item = encrypt_provider(
            &identity,
            ProviderSecret::new(String::from_utf8(key.to_vec()).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(&*decrypt_provider(&identity, &item).unwrap(), key);
        let last = item.len() - 1;
        item[last] ^= 1;
        assert!(decrypt_provider(&identity, &item).is_err());
        let last = encrypted.len() - 1;
        encrypted[last] ^= 1;
        assert!(unlock(&encrypted, passphrase()).is_err());
    }
}
