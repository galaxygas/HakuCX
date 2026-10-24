/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Release tooling (see docs/RELEASING.md).
//!
//! ```text
//! haku-release keygen <secret-key-file> <public-key-file>
//!     Create the update signing key. The password comes from HAKU_SIGNING_PASSWORD or is
//!     asked for. Keep the secret key out of the repository; commit the public key as
//!     packaging/update-key.pub.
//!
//! haku-release sign <file> <version>
//!     Write <file>.minisig, signed with the secret key in HAKU_SIGNING_KEY (the contents of
//!     the secret key file) and HAKU_SIGNING_PASSWORD. The trusted comment names the version
//!     and file, which the updater checks.
//!
//! haku-release verify <file> <version> <public-key-file>
//!     Check <file>.minisig the way the updater does.
//!
//! haku-release icon <out.ico>
//!     Write the HakuCX icon (for the installer).
//! ```

use std::io::Cursor;
use std::process::ExitCode;

fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("haku-release: {message}");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let password = std::env::var("HAKU_SIGNING_PASSWORD").ok();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["keygen", secret, public] => {
            if std::path::Path::new(secret).exists() || std::path::Path::new(public).exists() {
                return fail("refusing to overwrite an existing key file");
            }
            let kp = match minisign::KeyPair::generate_encrypted_keypair(password) {
                Ok(kp) => kp,
                Err(e) => return fail(e),
            };
            let pk = match kp.pk.to_box() {
                Ok(b) => b.to_string(),
                Err(e) => return fail(e),
            };
            let sk = match kp.sk.to_box(Some("HakuCX update signing key")) {
                Ok(b) => b.to_string(),
                Err(e) => return fail(e),
            };
            if let Err(e) = std::fs::write(secret, sk) {
                return fail(e);
            }
            if let Err(e) = std::fs::write(public, &pk) {
                return fail(e);
            }
            println!("Public key written to {public}:\n{pk}");
            println!("Keep {secret} and its password secret. Never commit them.");
            ExitCode::SUCCESS
        }
        ["sign", file, version] => {
            let Ok(key_text) = std::env::var("HAKU_SIGNING_KEY") else {
                return fail("HAKU_SIGNING_KEY is not set");
            };
            let sk = match minisign::SecretKeyBox::from_string(&key_text)
                .and_then(|b| b.into_secret_key(password))
            {
                Ok(sk) => sk,
                Err(e) => return fail(format!("the signing key could not be opened: {e}")),
            };
            let data = match std::fs::read(file) {
                Ok(d) => d,
                Err(e) => return fail(e),
            };
            let name = std::path::Path::new(file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let comment =
                haku_core::update::trusted_comment(version.trim_start_matches('v'), &name);
            let sig = match minisign::sign(
                None,
                &sk,
                Cursor::new(data),
                Some(&comment),
                Some("signature from the HakuCX release key"),
            ) {
                Ok(s) => s,
                Err(e) => return fail(e),
            };
            let out = format!("{file}.minisig");
            if let Err(e) = std::fs::write(&out, sig.to_string()) {
                return fail(e);
            }
            println!("{out}: {comment}");
            ExitCode::SUCCESS
        }
        ["verify", file, version, public] => {
            let (Ok(data), Ok(sig), Ok(pk)) = (
                std::fs::read(file),
                std::fs::read_to_string(format!("{file}.minisig")),
                std::fs::read_to_string(public),
            ) else {
                return fail("could not read the file, its .minisig, or the public key");
            };
            let name = std::path::Path::new(file)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match haku_core::update::verify(
                &data,
                &sig,
                &pk,
                version.trim_start_matches('v'),
                &name,
            ) {
                Ok(()) => {
                    println!("{file}: good signature for {version}");
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        ["icon", out] => match std::fs::write(out, haku_brand::ico(haku_brand::ICO_SIZES)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(e),
        },
        _ => fail(
            "usage: haku-release keygen <secret> <public> | sign <file> <version> | verify <file> <version> <public> | icon <out.ico>",
        ),
    }
}
