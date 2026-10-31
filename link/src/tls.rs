use rustls::{
    DigitallySignedStruct, DistinguishedName, Error, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::Zeroizing;

pub struct IdentityFiles {
    pub certificate: PathBuf,
    pub key: PathBuf,
    pub pin: crate::ManagerPin,
}

pub fn pin_from_certificate(der: &[u8]) -> io::Result<crate::ManagerPin> {
    let key = crate::IdentityPublicKey::from_certificate(der)
        .map_err(|_| io::Error::other("manager identity"))?;
    Ok(crate::ManagerPin::from_public_key(key.as_bytes()))
}

pub fn ensure_identity(directory: &Path, stem: &str) -> io::Result<IdentityFiles> {
    if stem.is_empty() || !stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(io::Error::other("invalid identity name"));
    }
    let certificate = directory.join(format!("{stem}.pem"));
    let key = directory.join(format!("{stem}.key"));
    match (certificate.exists(), key.exists()) {
        (true, true) => {
            check_private_file(&certificate)?;
            let certificates = certificates(&certificate)?;
            let pin = pin_from_certificate(certificates[0].as_ref())?;
            let _ = private_key(&key)?;
            Ok(IdentityFiles {
                certificate,
                key,
                pin,
            })
        }
        (false, false) => {
            let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)
                .map_err(|e| io::Error::other(e.to_string()))?;
            let mut params = rcgen::CertificateParams::new(Vec::<String>::new())
                .map_err(|e| io::Error::other(e.to_string()))?;
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, "botstral");
            params.extended_key_usages = vec![
                rcgen::ExtendedKeyUsagePurpose::ServerAuth,
                rcgen::ExtendedKeyUsagePurpose::ClientAuth,
            ];
            let cert = params
                .self_signed(&key_pair)
                .map_err(|e| io::Error::other(e.to_string()))?;
            let pin = pin_from_certificate(cert.der())?;
            write_private(&key, key_pair.serialize_pem().as_bytes())?;
            write_private(&certificate, cert.pem().as_bytes())?;
            Ok(IdentityFiles {
                certificate,
                key,
                pin,
            })
        }
        _ => Err(io::Error::other("incomplete identity")),
    }
}

fn algorithms() -> rustls::crypto::WebPkiSupportedAlgorithms {
    rustls::crypto::ring::default_provider().signature_verification_algorithms
}

#[derive(Debug)]
struct PinVerifier {
    pin: crate::CertificateFingerprint,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let pin = pin_from_certificate(end_entity.as_ref()).map_err(|_| {
            Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)
        })?;
        if pin.as_str() != self.pin.as_str() {
            return Err(Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &algorithms())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &algorithms())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        algorithms().supported_schemes()
    }
}

#[derive(Debug)]
struct ExtractEd25519;

impl ClientCertVerifier for ExtractEd25519 {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        false
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        crate::IdentityPublicKey::from_certificate(end_entity.as_ref())
            .map_err(|_| Error::InvalidCertificate(rustls::CertificateError::BadEncoding))?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &algorithms())
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &algorithms())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        algorithms().supported_schemes()
    }
}

fn pin_verifier(pin: &crate::ManagerPin) -> Arc<PinVerifier> {
    Arc::new(PinVerifier {
        pin: pin.fingerprint().clone(),
    })
}

pub fn pinned_manager_config(profile: &crate::ManagerTrust) -> io::Result<rustls::ClientConfig> {
    let pin = crate::ManagerPin::parse(profile.pin.as_str())
        .map_err(|_| io::Error::other("manager pin"))?;
    pinned_owner_config(&pin)
}

pub fn pinned_owner_config(pin: &crate::ManagerPin) -> io::Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(pin_verifier(pin))
        .with_no_client_auth())
}

pub fn pinned_worker_config(
    pin: &crate::ManagerPin,
    certificate: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> io::Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(pin_verifier(pin))
        .with_client_auth_cert(certificate, key)
        .map_err(io::Error::other)
}

pub fn public_server_config(
    certificate: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> io::Result<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(ExtractEd25519);
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certificate, key)
        .map_err(io::Error::other)
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub fn certificates(path: &Path) -> io::Result<Vec<CertificateDer<'static>>> {
    let certificates = rustls_pemfile::certs(&mut BufReader::new(File::open(path)?))
        .collect::<Result<Vec<_>, _>>()?;
    if certificates.is_empty() {
        return Err(io::Error::other("empty certificate chain"));
    }
    Ok(certificates)
}

pub fn check_private_file(path: &Path) -> io::Result<()> {
    private_file(path).map(|_| ())
}

fn private_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.mode() & 0o7777 != 0o600
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(io::Error::other("unsafe secret file"));
    }
    Ok(file)
}

pub fn private_bytes(path: &Path) -> io::Result<Zeroizing<Vec<u8>>> {
    let mut file = private_file(path)?;
    if file.metadata()?.len() > 64 * 1024 {
        return Err(io::Error::other("secret file too large"));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.by_ref().take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err(io::Error::other("secret file too large"));
    }
    Ok(bytes)
}

pub fn private_key(path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    let bytes = private_bytes(path)?;
    rustls_pemfile::private_key(&mut bytes.as_slice())?
        .ok_or_else(|| io::Error::other("missing private key"))
}
