use crate::{CertificateFingerprint, ErrorCode};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const PREFIX: &str = "bj1";
const HASH_DOMAIN: &[u8] = b"botstral-join-v1\0";
pub const ED25519_SPKI_PREFIX: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    Worker,
    Device,
}

impl PeerRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Device => "device",
        }
    }

    fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "worker" => Ok(Self::Worker),
            "device" => Ok(Self::Device),
            _ => Err(ErrorCode::InvalidInvitation),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerEndpoint(String);

impl ManagerEndpoint {
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        if value.is_empty()
            || value.len() > 259
            || value.contains(['/', '?', '#', '@', ' '])
            || !value.contains(':')
        {
            return Err(ErrorCode::InvalidInvitation);
        }
        let (host, port) = if let Some(rest) = value.strip_prefix('[') {
            let (host, tail) = rest.split_once("]:").ok_or(ErrorCode::InvalidInvitation)?;
            if host.is_empty() || host.contains(']') {
                return Err(ErrorCode::InvalidInvitation);
            }
            (host, tail)
        } else {
            value.rsplit_once(':').ok_or(ErrorCode::InvalidInvitation)?
        };
        if host.is_empty() || !host.bytes().all(|b| b.is_ascii()) {
            return Err(ErrorCode::InvalidInvitation);
        }
        let port: u16 = port.parse().map_err(|_| ErrorCode::InvalidInvitation)?;
        if port == 0 {
            return Err(ErrorCode::InvalidInvitation);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_wss(&self) -> String {
        format!("wss://{}/", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerPin(CertificateFingerprint);

impl ManagerPin {
    pub fn from_public_key(key: &[u8; 32]) -> Self {
        Self(CertificateFingerprint::from_public_key(key))
    }

    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        CertificateFingerprint::try_from(value.to_owned())
            .map(Self)
            .map_err(|_| ErrorCode::InvalidInvitation)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn fingerprint(&self) -> &CertificateFingerprint {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentityPublicKey([u8; 32]);

impl IdentityPublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_ed25519_spki(spki: &[u8]) -> Result<Self, ErrorCode> {
        if spki.len() == ED25519_SPKI_PREFIX.len() + 32 && spki.starts_with(ED25519_SPKI_PREFIX) {
            let mut key = [0; 32];
            key.copy_from_slice(&spki[ED25519_SPKI_PREFIX.len()..]);
            return Ok(Self(key));
        }
        Err(ErrorCode::InvalidInvitation)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn fingerprint(&self) -> CertificateFingerprint {
        CertificateFingerprint::from_public_key(&self.0)
    }

    pub fn from_certificate(der: &[u8]) -> Result<Self, ErrorCode> {
        let needle = ED25519_SPKI_PREFIX.len() + 32;
        der.windows(needle)
            .find(|window| window.starts_with(ED25519_SPKI_PREFIX))
            .ok_or(ErrorCode::InvalidInvitation)
            .and_then(|window| Self::from_ed25519_spki(&window[..needle]))
    }
}

pub struct JoinSecret(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for JoinSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JoinSecret")
    }
}

impl JoinSecret {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| ErrorCode::InvalidInvitation)?;
        if bytes.len() != 32 || URL_SAFE_NO_PAD.encode(&bytes) != value {
            return Err(ErrorCode::InvalidInvitation);
        }
        let mut key = [0; 32];
        key.copy_from_slice(&bytes);
        Ok(Self(Zeroizing::new(key)))
    }

    pub fn render(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(self.0.as_slice()))
    }

    pub fn hash(&self, role: PeerRole) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(HASH_DOMAIN);
        digest.update(role.as_str().as_bytes());
        digest.update([0]);
        digest.update(self.0.as_slice());
        digest.finalize().into()
    }
}

#[derive(Debug)]
pub struct JoinCode {
    role: PeerRole,
    endpoint: ManagerEndpoint,
    pin: ManagerPin,
    secret: JoinSecret,
}

impl JoinCode {
    pub fn new(
        role: PeerRole,
        endpoint: ManagerEndpoint,
        pin: ManagerPin,
        secret: JoinSecret,
    ) -> Self {
        Self {
            role,
            endpoint,
            pin,
            secret,
        }
    }

    pub fn parse(raw: &str) -> Result<Self, ErrorCode> {
        let raw = raw.strip_suffix('\n').unwrap_or(raw);
        if raw.len() > 512 || raw.as_bytes().iter().any(u8::is_ascii_whitespace) {
            return Err(ErrorCode::InvalidInvitation);
        }
        let mut parts = raw.split('/');
        if parts.next() != Some(PREFIX) {
            return Err(ErrorCode::InvalidInvitation);
        }
        let role = PeerRole::parse(parts.next().ok_or(ErrorCode::InvalidInvitation)?)?;
        let endpoint = ManagerEndpoint::parse(parts.next().ok_or(ErrorCode::InvalidInvitation)?)?;
        let pin = ManagerPin::parse(parts.next().ok_or(ErrorCode::InvalidInvitation)?)?;
        let secret = JoinSecret::parse(parts.next().ok_or(ErrorCode::InvalidInvitation)?)?;
        if parts.next().is_some() {
            return Err(ErrorCode::InvalidInvitation);
        }
        Ok(Self {
            role,
            endpoint,
            pin,
            secret,
        })
    }

    pub fn render(&self) -> Zeroizing<String> {
        let secret = self.secret.render();
        Zeroizing::new(format!(
            "{PREFIX}/{}/{}/{}/{}",
            self.role.as_str(),
            self.endpoint.as_str(),
            self.pin.as_str(),
            secret.as_str()
        ))
    }

    pub fn role(&self) -> PeerRole {
        self.role
    }

    pub fn endpoint(&self) -> &ManagerEndpoint {
        &self.endpoint
    }

    pub fn pin(&self) -> &ManagerPin {
        &self.pin
    }

    pub fn secret_hash(&self) -> [u8; 32] {
        self.secret.hash(self.role)
    }

    pub fn secret_text(&self) -> Zeroizing<String> {
        self.secret.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> JoinCode {
        JoinCode::new(
            PeerRole::Worker,
            ManagerEndpoint::parse("manager.example:443").unwrap(),
            ManagerPin::from_public_key(&[7; 32]),
            JoinSecret::from_bytes([9; 32]),
        )
    }

    #[test]
    fn render_then_parse_is_stable() {
        let code = sample();
        let rendered = code.render();
        let parsed = JoinCode::parse(rendered.as_str()).unwrap();
        assert_eq!(parsed.role(), PeerRole::Worker);
        assert_eq!(parsed.endpoint().as_str(), "manager.example:443");
        assert_eq!(parsed.pin().as_str(), code.pin().as_str());
        assert_eq!(parsed.secret_hash(), code.secret_hash());
        assert_eq!(parsed.render().as_str(), rendered.as_str());
    }

    #[test]
    fn parse_rejects_tampering() {
        let rendered = sample().render();
        let pin = sample().pin().as_str().to_owned();
        let broken = rendered.as_str().replacen(&pin, "g", 1);
        assert_eq!(
            JoinCode::parse(&broken).unwrap_err(),
            ErrorCode::InvalidInvitation
        );
        assert_eq!(
            JoinCode::parse("nope").unwrap_err(),
            ErrorCode::InvalidInvitation
        );
    }
}
