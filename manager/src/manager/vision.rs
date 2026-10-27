use super::OperationError;
use botstral_link::*;
use std::{
    fs,
    io::{Cursor, Read},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub(super) fn image_bytes(
    directory: &Path,
    reference: &ImageReference,
    length: u64,
) -> Result<(Vec<u8>, u32, u32), OperationError> {
    let invalid = OperationError::InvalidInput;
    if length == 0 || length > MAX_IMAGE_BYTES {
        return Err(invalid);
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(
            directory
                .join("artifacts")
                .join(format!("{}.blob", reference.artifact)),
        )
        .map_err(|_| invalid)?;
    let meta = file.metadata().map_err(|_| invalid)?;
    if !meta.is_file() || meta.len() != length {
        return Err(invalid);
    }
    let mut bytes = Vec::new();
    file.take(length + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid)?;
    if bytes.len() as u64 != length
        || CertificateFingerprint::from_der(&bytes) != reference.sha256
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    {
        return Err(invalid);
    }
    let decoder = png::Decoder::new_with_limits(
        Cursor::new(&bytes),
        png::Limits {
            bytes: 16 * 1024 * 1024,
        },
    );
    let mut reader = decoder.read_info().map_err(|_| invalid)?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
        || info.animation_control.is_some()
        || reader.output_buffer_size() > 16 * 1024 * 1024
    {
        return Err(invalid);
    }
    let mut decoded = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut decoded).map_err(|_| invalid)?;
    reader.finish().map_err(|_| invalid)?;
    Ok((bytes, width, height))
}
