//! Checks who published a checksum file.
//!
//! A checksum only proves that a file arrived whole: it comes from the same
//! place as the file, so whoever could replace one could replace both.
//! yt-dlp also signs its checksum file (`SHA2-256SUMS.sig`, a detached
//! OpenPGP signature), and the key it must be signed with is compiled into
//! this application ([`super::platform::SignedBy`]).

use std::io::Cursor;

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::types::KeyDetails;

use super::Error;
use super::platform::SignedBy;

/// `Ok` only when `signature` is a valid signature of `data` by the key in
/// `signer`, and that key really has the fingerprint `signer` expects.
pub fn verify(data: &[u8], signature: &[u8], signer: &SignedBy) -> Result<(), Error> {
    let key = trusted_key(signer)?;
    let signature = DetachedSignature::from_bytes(Cursor::new(signature))
        .map_err(|e| refused(format!("the signature can't be read ({e})")))?;
    // Normally made with the primary key; a signing subkey is accepted too.
    let by_primary = signature.verify(&key, data).is_ok();
    let by_subkey = || {
        key.public_subkeys
            .iter()
            .any(|subkey| signature.verify(subkey, data).is_ok())
    };
    if by_primary || by_subkey() {
        Ok(())
    } else {
        Err(refused(
            "the checksum file is not signed by the expected key".into(),
        ))
    }
}

/// The compiled-in key, after checking it is the one its fingerprint names:
/// a swapped key file must not quietly become the new authority.
fn trusted_key(signer: &SignedBy) -> Result<SignedPublicKey, Error> {
    let (key, _) = SignedPublicKey::from_armor_single(Cursor::new(signer.key))
        .map_err(|e| refused(format!("the signing key can't be read ({e})")))?;
    let fingerprint = format!("{:x}", key.fingerprint());
    if !fingerprint.eq_ignore_ascii_case(signer.fingerprint) {
        return Err(refused(format!(
            "the signing key is {fingerprint}, not {}",
            signer.fingerprint
        )));
    }
    key.verify_bindings()
        .map_err(|e| refused(format!("the signing key is not valid ({e})")))?;
    Ok(key)
}

fn refused(reason: String) -> Error {
    Error::Checksum(format!("Refusing to trust the checksums: {reason}."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provision::platform::YTDLP_SIGNED_BY;

    const SUMS: &[u8] = include_bytes!("testdata/ytdlp-SHA2-256SUMS");
    const SIG: &[u8] = include_bytes!("testdata/ytdlp-SHA2-256SUMS.sig");

    /// yt-dlp's real SHA2-256SUMS and signature from the 2026.09.16.232951
    /// nightly, which `gpg --verify` accepted as signed by this key.
    #[test]
    fn yt_dlps_real_checksum_file_verifies() {
        verify(SUMS, SIG, &YTDLP_SIGNED_BY).unwrap();
    }

    #[test]
    fn one_changed_byte_in_the_checksums_is_caught() {
        let mut tampered = SUMS.to_vec();
        tampered[0] = if tampered[0] == b'0' { b'1' } else { b'0' };
        assert!(verify(&tampered, SIG, &YTDLP_SIGNED_BY).is_err());
    }

    #[test]
    fn a_damaged_signature_is_refused() {
        assert!(verify(SUMS, &SIG[..SIG.len() / 2], &YTDLP_SIGNED_BY).is_err());
        assert!(verify(SUMS, b"not a signature", &YTDLP_SIGNED_BY).is_err());
    }

    /// The fingerprint is checked, so replacing the key file alone (with a
    /// key an attacker signed with) is not enough.
    #[test]
    fn a_key_that_does_not_match_its_fingerprint_is_not_trusted() {
        let swapped = SignedBy {
            key: include_str!("testdata/test-key.asc"),
            ..YTDLP_SIGNED_BY
        };
        let err = verify(
            include_bytes!("testdata/test-SUMS"),
            include_bytes!("testdata/test-SUMS.sig"),
            &swapped,
        )
        .unwrap_err();
        assert!(err.to_string().contains("signing key is"), "{err}");
    }
}
