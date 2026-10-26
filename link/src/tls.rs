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
    file.read_to_end(&mut bytes)?;
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
