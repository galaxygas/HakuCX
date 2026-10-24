/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Updates: HakuCX releases are published on GitHub with a minisign signature for each
//! installer. The browser asks GitHub for the latest release, downloads the installer and its
//! signature, and runs it only if the signature verifies with the public key built into
//! HakuCX *and* the signed comment names that exact version and file (so an old, validly
//! signed installer cannot be passed off as an update).

use serde::Deserialize;

pub const REPO: &str = "galaxygas/HakuCX";
pub const LATEST_URL: &str = "https://api.github.com/repos/galaxygas/HakuCX/releases/latest";

/// The installer's file name for a version.
pub fn installer_name(version: &str) -> String {
    format!("HakuCX-Setup-{version}-x64.exe")
}

/// The trusted comment a release's signature must carry.
pub fn trusted_comment(version: &str, file: &str) -> String {
    format!("hakucx {version} {file}")
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum UpdateError {
    #[error("the release information could not be read")]
    BadRelease,
    #[error("the release has no installer for this system")]
    NoInstaller,
    #[error("the update's signature is not valid")]
    BadSignature,
    #[error("the update's signature is for a different file or version")]
    WrongFile,
    #[error("this build has no update key")]
    NoKey,
}

/// What HakuCX needs from GitHub's "latest release" answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub installer_url: String,
    pub signature_url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Parse `GET /repos/{repo}/releases/latest`. Download links must point at GitHub.
pub fn parse_latest(json: &str) -> Result<Release, UpdateError> {
    let r: GhRelease = serde_json::from_str(json).map_err(|_| UpdateError::BadRelease)?;
    if r.draft || r.prerelease {
        return Err(UpdateError::BadRelease);
    }
    let version = r.tag_name.trim_start_matches('v').to_owned();
    parse_version(&version).ok_or(UpdateError::BadRelease)?;
    let installer = installer_name(&version);
    let find = |name: &str| r.assets.iter().find(|a| a.name == name);
    let asset = find(&installer).ok_or(UpdateError::NoInstaller)?;
    let sig = find(&format!("{installer}.minisig")).ok_or(UpdateError::NoInstaller)?;
    let github = |u: &str| {
        u.starts_with("https://github.com/")
            || u.starts_with("https://objects.githubusercontent.com/")
    };
    if !github(&asset.browser_download_url) || !github(&sig.browser_download_url) {
        return Err(UpdateError::BadRelease);
    }
    Ok(Release {
        version,
        page: r.html_url,
        installer_url: asset.browser_download_url.clone(),
        signature_url: sig.browser_download_url.clone(),
        size: asset.size,
    })
}

/// `major.minor.patch` with an optional `-pre` suffix (pre-releases sort first).
fn parse_version(v: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p.to_owned())),
        None => (v, None),
    };
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??, pre);
    parts.next().is_none().then_some(version)
}

/// Is `candidate` a newer version than `current`?
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => {
            let core = (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2));
            match core {
                std::cmp::Ordering::Equal => match (&a.3, &b.3) {
                    (None, Some(_)) => true,
                    (Some(x), Some(y)) => x > y,
                    _ => false,
                },
                o => o == std::cmp::Ordering::Greater,
            }
        }
        _ => false,
    }
}

/// Check a downloaded installer against its minisign signature and HakuCX's public key
/// (the second line of a minisign `.pub` file).
pub fn verify(
    installer: &[u8],
    signature: &str,
    public_key: &str,
    version: &str,
    file: &str,
) -> Result<(), UpdateError> {
    let key = public_key
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
        .ok_or(UpdateError::NoKey)?;
    let pk = minisign_verify::PublicKey::from_base64(key).map_err(|_| UpdateError::NoKey)?;
    let sig =
        minisign_verify::Signature::decode(signature).map_err(|_| UpdateError::BadSignature)?;
    pk.verify(installer, &sig, false)
        .map_err(|_| UpdateError::BadSignature)?;
    if sig.trusted_comment() != trusted_comment(version, file) {
        return Err(UpdateError::WrongFile);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.2.0", "0.2.0-beta"));
        assert!(!is_newer("0.2.0-beta", "0.2.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert!(!is_newer("1.2", "0.1.0"));
    }

    #[test]
    fn github_release() {
        let json = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/galaxygas/HakuCX/releases/tag/v0.2.0","draft":false,"prerelease":false,
          "assets":[{"name":"HakuCX-Setup-0.2.0-x64.exe","browser_download_url":"https://github.com/galaxygas/HakuCX/releases/download/v0.2.0/HakuCX-Setup-0.2.0-x64.exe","size":5},
                    {"name":"HakuCX-Setup-0.2.0-x64.exe.minisig","browser_download_url":"https://github.com/galaxygas/HakuCX/releases/download/v0.2.0/HakuCX-Setup-0.2.0-x64.exe.minisig","size":1}]}"#;
        let r = parse_latest(json).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert!(r.installer_url.ends_with("HakuCX-Setup-0.2.0-x64.exe"));
        let elsewhere = json.replace("https://github.com/galaxygas/HakuCX/releases/download/v0.2.0/HakuCX-Setup-0.2.0-x64.exe\"", "https://evil.test/x.exe\"");
        assert_eq!(
            parse_latest(&elsewhere).err(),
            Some(UpdateError::BadRelease)
        );
        assert_eq!(
            parse_latest(&json.replace("\"prerelease\":false", "\"prerelease\":true")).err(),
            Some(UpdateError::BadRelease)
        );
        assert_eq!(
            parse_latest(&json.replace("Setup-0.2.0-x64.exe.minisig", "other")).err(),
            Some(UpdateError::NoInstaller)
        );
    }

    #[test]
    fn signatures() {
        use std::io::Cursor;
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public = kp.pk.to_box().unwrap().to_string();
        let data = b"installer bytes".to_vec();
        let sign = |comment: &str| {
            minisign::sign(
                Some(&kp.pk),
                &kp.sk,
                Cursor::new(&data),
                Some(comment),
                Some("signature from the HakuCX release key"),
            )
            .unwrap()
            .to_string()
        };
        let file = installer_name("0.2.0");
        let good = sign(&trusted_comment("0.2.0", &file));
        assert_eq!(verify(&data, &good, &public, "0.2.0", &file), Ok(()));
        // A different file or a tampered installer fail.
        assert_eq!(
            verify(b"other bytes", &good, &public, "0.2.0", &file),
            Err(UpdateError::BadSignature)
        );
        // An old installer's valid signature cannot pose as a newer version.
        let old = sign(&trusted_comment("0.1.0", &installer_name("0.1.0")));
        assert_eq!(
            verify(&data, &old, &public, "0.2.0", &file),
            Err(UpdateError::WrongFile)
        );
        // Another key fails.
        let other = minisign::KeyPair::generate_unencrypted_keypair()
            .unwrap()
            .pk
            .to_box()
            .unwrap()
            .to_string();
        assert_eq!(
            verify(&data, &good, &other, "0.2.0", &file),
            Err(UpdateError::BadSignature)
        );
        assert_eq!(
            verify(&data, &good, "", "0.2.0", &file),
            Err(UpdateError::NoKey)
        );
    }
}
