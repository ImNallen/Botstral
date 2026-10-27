use rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer},
};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use zeroize::Zeroizing;

fn trust_roots(profile: &crate::ManagerTrust) -> io::Result<RootCertStore> {
    if profile.ca_pem.len() > 32768
        || profile.server_name.len() > 253
        || profile.endpoint.len() > 512
        || profile.endpoint.is_empty()
        || profile.endpoint.contains(['/', '?', '#', '@'])
    {
        return Err(io::Error::other("invalid manager trust"));
    }
    let mut roots = RootCertStore::empty();
    for certificate in rustls_pemfile::certs(&mut profile.ca_pem.as_bytes()) {
        roots.add(certificate?).map_err(io::Error::other)?;
    }
    Ok(roots)
}

pub fn validate_manager_trust(
    profile: &crate::ManagerTrust,
    certificates: &[CertificateDer<'_>],
) -> io::Result<()> {
    use rustls::{
        client::{WebPkiServerVerifier, danger::ServerCertVerifier},
        pki_types::{ServerName, UnixTime},
    };
    let verifier = WebPkiServerVerifier::builder_with_provider(
        std::sync::Arc::new(trust_roots(profile)?),
        std::sync::Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .map_err(io::Error::other)?;
    let leaf = certificates
        .first()
        .ok_or_else(|| io::Error::other("missing manager certificate"))?;
    if crate::CertificateFingerprint::from_der(leaf.as_ref()) != profile.leaf {
        return Err(io::Error::other("manager pin mismatch"));
    }
    let name = ServerName::try_from(profile.server_name.clone()).map_err(io::Error::other)?;
    verifier
        .verify_server_cert(leaf, &certificates[1..], &name, &[], UnixTime::now())
        .map_err(io::Error::other)?;
    Ok(())
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

pub fn roots(path: &Path) -> io::Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    for certificate in certificates(path)? {
        roots.add(certificate).map_err(io::Error::other)?;
    }
    Ok(roots)
}
