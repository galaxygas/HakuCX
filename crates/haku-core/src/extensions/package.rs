/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Extension packages: Chrome `.crx` (versions 2 and 3), Firefox `.xpi` and plain `.zip`
//! archives, and extension ids.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("this is not an extension package (expected .crx, .xpi or .zip)")]
    NotAPackage,
    #[error("the package is damaged: {0}")]
    Damaged(String),
    #[error("the package is too large")]
    TooLarge,
    #[error("the package's signature is not valid: {0}")]
    BadSignature(&'static str),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Total unpacked size we accept (uncompressed), to bound zip bombs.
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
const MAX_FILES: usize = 20_000;

/// What the package header says, beyond the zip contents.
#[derive(Debug, Default, PartialEq)]
pub struct PackageInfo {
    /// For `.crx`: the 32-character id from the signed header (CRX3) or the public key (CRX2).
    pub crx_id: Option<String>,
}

/// The zip archive inside a package, and the header information. A `.crx` must carry valid
/// signatures (see [`crx3_verify`]); `.xpi` and `.zip` archives are unsigned here.
pub fn zip_payload(bytes: &[u8]) -> Result<(&[u8], PackageInfo), PackageError> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Ok((bytes, PackageInfo::default()));
    }
    if !bytes.starts_with(b"Cr24") || bytes.len() < 16 {
        return Err(PackageError::NotAPackage);
    }
    let u32_at = |i: usize| -> Result<usize, PackageError> {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| PackageError::Damaged("truncated header".into()))
    };
    match u32_at(4)? {
        2 => {
            let key_len = u32_at(8)?;
            let sig_len = u32_at(12)?;
            let start = 16 + key_len + sig_len;
            let key = bytes
                .get(16..16 + key_len)
                .ok_or_else(|| PackageError::Damaged("truncated key".into()))?;
            let signature = bytes
                .get(16 + key_len..start)
                .ok_or_else(|| PackageError::Damaged("truncated signature".into()))?;
            let zip = bytes
                .get(start..)
                .ok_or_else(|| PackageError::Damaged("truncated".into()))?;
            // CRX2 signs the archive with RSA and SHA-1, the key being the extension's own.
            use aws_lc_rs::signature::{
                RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY, UnparsedPublicKey,
            };
            UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY, key)
                .verify(zip, signature)
                .map_err(|_| {
                    PackageError::BadSignature("the archive does not match its signature")
                })?;
            Ok((
                zip,
                PackageInfo {
                    crx_id: Some(id_from_key_bytes(key)),
                },
            ))
        }
        3 => {
            let header_len = u32_at(8)?;
            let header = bytes
                .get(12..12 + header_len)
                .ok_or_else(|| PackageError::Damaged("truncated header".into()))?;
            let zip = bytes
                .get(12 + header_len..)
                .ok_or_else(|| PackageError::Damaged("truncated".into()))?;
            let crx_id = crx3_verify(header, zip)?;
            Ok((
                zip,
                PackageInfo {
                    crx_id: Some(crx_id),
                },
            ))
        }
        v => Err(PackageError::Damaged(format!("unknown CRX version {v}"))),
    }
}

/// Read `manifest.json` from a package without extracting it.
pub fn read_manifest(bytes: &[u8]) -> Result<String, PackageError> {
    let (zip, _) = zip_payload(bytes)?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| PackageError::Damaged(e.to_string()))?;
    let mut entry = archive
        .by_name("manifest.json")
        .map_err(|_| PackageError::Damaged("no manifest.json".into()))?;
    let mut text = String::new();
    (&mut entry)
        .take(4 * 1024 * 1024)
        .read_to_string(&mut text)?;
    Ok(text)
}

/// Extract a package into `dest` (which must not exist yet). Paths that would escape `dest`,
/// absolute paths and oversized archives are refused.
pub fn extract(bytes: &[u8], dest: &Path) -> Result<PackageInfo, PackageError> {
    let (zip, info) = zip_payload(bytes)?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| PackageError::Damaged(e.to_string()))?;
    if archive.len() > MAX_FILES {
        return Err(PackageError::TooLarge);
    }
    let mut total = 0u64;
    std::fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| PackageError::Damaged(e.to_string()))?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(PackageError::Damaged(format!(
                "unsafe path {}",
                String::from_utf8_lossy(entry.name_raw())
            )));
        };
        let out = dest.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > MAX_UNPACKED {
            return Err(PackageError::TooLarge);
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let declared = entry.size();
        let mut data = Vec::with_capacity(declared.min(64 * 1024 * 1024) as usize);
        // Read at most the declared size + 1 so a lying header cannot inflate unbounded.
        (&mut entry).take(declared + 1).read_to_end(&mut data)?;
        if data.len() as u64 > declared {
            return Err(PackageError::Damaged("entry larger than declared".into()));
        }
        std::fs::write(&out, &data)?;
    }
    Ok(info)
}

/// Chrome's id alphabet: hex digits mapped to `a`..`p`.
fn to_chrome_id(hash: &[u8]) -> String {
    hash.iter()
        .take(16)
        .flat_map(|b| [b >> 4, b & 0x0f])
        .map(|n| (b'a' + n) as char)
        .collect()
}

pub fn id_from_key_bytes(key: &[u8]) -> String {
    to_chrome_id(&Sha256::digest(key))
}

/// The id Chrome gives an extension whose manifest has a `key` (base64 public key).
pub fn id_from_manifest_key(key: &str) -> Option<String> {
    let cleaned: String = key.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .ok()?;
    Some(id_from_key_bytes(&bytes))
}

/// The id Chrome gives an unpacked extension: derived from its folder path.
pub fn id_from_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    // Chrome hashes the UTF-16 path on Windows; any stable hash works for our own ids, but use
    // the same input so ids match between browsers for developers.
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    to_chrome_id(&Sha256::digest(&utf16))
}

/// A stable id for a Firefox extension, from its gecko id.
pub fn id_from_name(name: &str) -> String {
    to_chrome_id(&Sha256::digest(name.as_bytes()))
}

pub fn is_valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Check a CRX3 header against the archive the way Chrome does for packages from outside its
/// store, and return the extension id. The header is a protobuf `CrxFileHeader`:
/// `sha256_with_rsa` (2) and `sha256_with_ecdsa` (3) are repeated `AsymmetricKeyProof`s
/// (`public_key` = 1, `signature` = 2), and `signed_header_data` (10000) is a `SignedData`
/// whose field 1 is the 16-byte crx id. Every proof must verify over
/// `"CRX3 SigningContext\0" ‖ len(signed_header_data) as u32 LE ‖ signed_header_data ‖ archive`,
/// and one proof's key must be the key the id is derived from (so an update can only come from
/// whoever holds that key).
pub fn crx3_verify(header: &[u8], archive: &[u8]) -> Result<String, PackageError> {
    use aws_lc_rs::signature::{
        ECDSA_P256_SHA256_ASN1, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey,
        VerificationAlgorithm,
    };
    let signed = protobuf_fields(header, 10000)
        .and_then(|f| f.first().copied())
        .ok_or(PackageError::BadSignature("no signed header"))?;
    let id = protobuf_fields(signed, 1)
        .and_then(|f| f.first().copied())
        .filter(|id| id.len() == 16)
        .ok_or(PackageError::BadSignature(
            "no extension id in the signed header",
        ))?;
    let mut message = b"CRX3 SigningContext\0".to_vec();
    message.extend_from_slice(&(signed.len() as u32).to_le_bytes());
    message.extend_from_slice(signed);
    message.extend_from_slice(archive);
    let damaged = || PackageError::Damaged("unreadable signature header".into());
    let algorithms: [(u64, &'static dyn VerificationAlgorithm); 2] = [
        (2, &RSA_PKCS1_2048_8192_SHA256),
        (3, &ECDSA_P256_SHA256_ASN1),
    ];
    let mut proofs = 0;
    let mut developer_key = false;
    for (field, algorithm) in algorithms {
        for proof in protobuf_fields(header, field).ok_or_else(damaged)? {
            let key = protobuf_fields(proof, 1)
                .and_then(|f| f.first().copied())
                .ok_or_else(damaged)?;
            let signature = protobuf_fields(proof, 2)
                .and_then(|f| f.first().copied())
                .ok_or_else(damaged)?;
            UnparsedPublicKey::new(algorithm, key)
                .verify(&message, signature)
                .map_err(|_| {
                    PackageError::BadSignature("the archive does not match its signature")
                })?;
            if Sha256::digest(key)[..16] == *id {
                developer_key = true;
            }
            proofs += 1;
        }
    }
    if proofs == 0 {
        return Err(PackageError::BadSignature("the package is not signed"));
    }
    if !developer_key {
        return Err(PackageError::BadSignature(
            "no signature from the key its extension id belongs to",
        ));
    }
    Ok(to_chrome_id(id))
}

/// Every length-delimited field `wanted` in a protobuf message, or `None` if it is malformed.
fn protobuf_fields(mut buf: &[u8], wanted: u64) -> Option<Vec<&[u8]>> {
    fn varint(buf: &mut &[u8]) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&b, rest) = buf.split_first()?;
            *buf = rest;
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
    let mut out = Vec::new();
    while !buf.is_empty() {
        let key = varint(&mut buf)?;
        let (field, wire) = (key >> 3, key & 7);
        match wire {
            0 => {
                varint(&mut buf)?;
            }
            1 => buf = buf.get(8..)?,
            2 => {
                let len = usize::try_from(varint(&mut buf)?).ok()?;
                let data = buf.get(..len)?;
                buf = &buf[len..];
                if field == wanted {
                    out.push(data);
                }
            }
            5 => buf = buf.get(4..)?,
            _ => return None,
        }
    }
    Some(out)
}

/// Copy an unpacked extension folder (for "Load unpacked" we use the folder in place; this is
/// for installing from a folder as a copy).
pub fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target: PathBuf = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    use aws_lc_rs::encoding::AsDer;
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::rsa::KeySize;
    use aws_lc_rs::signature::{
        ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair, RSA_PKCS1_SHA256, RsaKeyPair,
    };

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, text) in files {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(text.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn extracts_zip_and_crx3() {
        let zip = zip_with(&[("manifest.json", "{}"), ("js/a.js", "1")]);
        let dir = tempfile::tempdir().unwrap();
        extract(&zip, &dir.path().join("x")).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("x/js/a.js")).unwrap(),
            "1"
        );

        // A CRX3 signed with an RSA key: the id comes from that key.
        let rsa = RsaKeyPair::generate(KeySize::Rsa2048).unwrap();
        let rsa_key = rsa.public_key().as_der().unwrap().as_ref().to_vec();
        let crx = signed_crx3(&zip, Some(&rsa), None, &rsa_key);
        let info = extract(&crx, &dir.path().join("y")).unwrap();
        assert_eq!(info.crx_id, Some(id_from_key_bytes(&rsa_key)));
        assert!(dir.path().join("y/manifest.json").exists());
        assert_eq!(read_manifest(&crx).unwrap(), "{}");
    }

    fn proto(field: u64, data: &[u8]) -> Vec<u8> {
        fn varint(mut v: u64, out: &mut Vec<u8>) {
            while v >= 0x80 {
                out.push((v as u8) | 0x80);
                v >>= 7;
            }
            out.push(v as u8);
        }
        let mut out = Vec::new();
        varint((field << 3) | 2, &mut out);
        varint(data.len() as u64, &mut out);
        out.extend_from_slice(data);
        out
    }

    /// A CRX3 package whose id is derived from `id_key`, signed by the given keys.
    fn signed_crx3(
        zip: &[u8],
        rsa: Option<&RsaKeyPair>,
        ec: Option<&EcdsaKeyPair>,
        id_key: &[u8],
    ) -> Vec<u8> {
        let rng = SystemRandom::new();
        let signed = proto(1, &Sha256::digest(id_key)[..16]);
        let mut message = b"CRX3 SigningContext\0".to_vec();
        message.extend_from_slice(&(signed.len() as u32).to_le_bytes());
        message.extend_from_slice(&signed);
        message.extend_from_slice(zip);
        let mut header = Vec::new();
        if let Some(key) = rsa {
            let mut sig = vec![0; key.public_modulus_len()];
            key.sign(&RSA_PKCS1_SHA256, &rng, &message, &mut sig)
                .unwrap();
            let public = key.public_key().as_der().unwrap();
            header.extend(proto(
                2,
                &[proto(1, public.as_ref()), proto(2, &sig)].concat(),
            ));
        }
        if let Some(key) = ec {
            let sig = key.sign(&rng, &message).unwrap();
            let public = key.public_key().as_der().unwrap();
            header.extend(proto(
                3,
                &[proto(1, public.as_ref()), proto(2, sig.as_ref())].concat(),
            ));
        }
        header.extend(proto(10000, &signed));
        let mut crx = b"Cr24".to_vec();
        crx.extend_from_slice(&3u32.to_le_bytes());
        crx.extend_from_slice(&(header.len() as u32).to_le_bytes());
        crx.extend_from_slice(&header);
        crx.extend_from_slice(zip);
        crx
    }

    #[test]
    fn crx_signatures_are_checked() {
        let zip = zip_with(&[("manifest.json", "{}")]);
        let rsa = RsaKeyPair::generate(KeySize::Rsa2048).unwrap();
        let rsa_key = rsa.public_key().as_der().unwrap().as_ref().to_vec();
        let ec = EcdsaKeyPair::generate(&ECDSA_P256_SHA256_ASN1_SIGNING).unwrap();
        let ec_key = ec.public_key().as_der().unwrap().as_ref().to_vec();

        // Both kinds of proof, id from either key.
        assert!(zip_payload(&signed_crx3(&zip, Some(&rsa), Some(&ec), &rsa_key)).is_ok());
        let (_, info) = zip_payload(&signed_crx3(&zip, None, Some(&ec), &ec_key)).unwrap();
        assert_eq!(info.crx_id, Some(id_from_key_bytes(&ec_key)));

        // A changed archive fails.
        let mut tampered = signed_crx3(&zip, Some(&rsa), None, &rsa_key);
        let last = tampered.len() - 30;
        tampered[last] ^= 0xff;
        assert!(matches!(
            zip_payload(&tampered),
            Err(PackageError::BadSignature(_))
        ));

        // Claiming an id whose key did not sign the package fails.
        let other = EcdsaKeyPair::generate(&ECDSA_P256_SHA256_ASN1_SIGNING).unwrap();
        let other_key = other.public_key().as_der().unwrap().as_ref().to_vec();
        assert!(matches!(
            zip_payload(&signed_crx3(&zip, Some(&rsa), None, &other_key)),
            Err(PackageError::BadSignature(_))
        ));

        // No proofs at all fails.
        assert!(matches!(
            zip_payload(&signed_crx3(&zip, None, None, &rsa_key)),
            Err(PackageError::BadSignature(_))
        ));
    }

    #[test]
    fn refuses_escaping_paths_and_junk() {
        let zip = zip_with(&[("../evil.txt", "x")]);
        let dir = tempfile::tempdir().unwrap();
        assert!(extract(&zip, &dir.path().join("x")).is_err());
        assert!(!dir.path().join("evil.txt").exists());
        assert!(matches!(
            extract(b"hello", &dir.path().join("z")),
            Err(PackageError::NotAPackage)
        ));
    }

    #[test]
    fn ids() {
        // Known value: Chrome id for the empty key is the SHA-256 of nothing.
        assert_eq!(id_from_key_bytes(b""), "odlameecjipmbmbejkplpemijjgpljce");
        assert!(is_valid_id(&id_from_name("ublock@example.org")));
        assert!(!is_valid_id("ABC"));
        assert_eq!(
            id_from_manifest_key("AAAA"),
            Some(id_from_key_bytes(&[0, 0, 0]))
        );
    }
}
