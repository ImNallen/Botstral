use super::*;

pub(super) fn decode_seed(seed: &str) -> Result<Zeroizing<Vec<u8>>, OperationError> {
    if !(32..=128).contains(&seed.len()) {
        return Err(OperationError::InvalidInput);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut buffer = 0u16;
    let mut bits = 0;
    for character in seed.bytes() {
        let value = match character {
            b'A'..=b'Z' => character - b'A',
            b'2'..=b'7' => character - b'2' + 26,
            _ => return Err(OperationError::InvalidInput),
        };
        buffer = (buffer << 5) | u16::from(value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
        buffer &= (1 << bits) - 1;
    }
    if buffer != 0 || ![0, 2, 4, 5, 7].contains(&(seed.len() % 8)) {
        return Err(OperationError::InvalidInput);
    }
    Ok(bytes)
}

#[cfg(test)]
fn truncate(seed: &[u8], time: u64) -> u32 {
    use hmac::{Hmac, Mac};
    let mut mac =
        Hmac::<sha1::Sha1>::new_from_slice(seed).expect("HMAC accepts arbitrary key lengths");
    mac.update(&(time / 30).to_be_bytes());
    let mut digest = Zeroizing::new(mac.finalize().into_bytes().to_vec());
    let offset = usize::from(digest[19] & 15);
    let value = u32::from_be_bytes(
        digest[offset..offset + 4]
            .try_into()
            .expect("bounded SHA-1 offset"),
    ) & 0x7fff_ffff;
    digest.clear();
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc6238_appendix_b_sha1_vectors() {
        for (time, expected) in [
            (59, 94287082),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
            (20000000000, 65353130),
        ] {
            assert_eq!(
                truncate(b"12345678901234567890", time) % 100_000_000,
                expected
            );
        }
        assert_eq!(
            &*decode_seed("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap(),
            b"12345678901234567890"
        );
        for seed in [
            "",
            "gezdgnbvgy3tqojqgezdgnbvgy3tqojq",
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ=",
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQA",
        ] {
            assert!(decode_seed(seed).is_err());
        }
    }
}
