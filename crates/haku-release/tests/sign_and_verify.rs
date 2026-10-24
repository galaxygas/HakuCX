/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The release tool's signatures are what the browser's updater accepts.

use std::process::Command;

#[test]
fn keygen_sign_and_verify() {
    let dir = tempfile::tempdir().unwrap();
    let (secret, public) = (dir.path().join("key.secret"), dir.path().join("key.pub"));
    let tool = env!("CARGO_BIN_EXE_haku-release");
    let status = Command::new(tool)
        .args(["keygen", secret.to_str().unwrap(), public.to_str().unwrap()])
        .env("HAKU_SIGNING_PASSWORD", "test password")
        .status()
        .unwrap();
    assert!(status.success());
    // Refuses to overwrite.
    assert!(
        !Command::new(tool)
            .args(["keygen", secret.to_str().unwrap(), public.to_str().unwrap()])
            .env("HAKU_SIGNING_PASSWORD", "x")
            .status()
            .unwrap()
            .success()
    );

    let name = haku_core::update::installer_name("0.3.0");
    let file = dir.path().join(&name);
    std::fs::write(&file, b"pretend installer").unwrap();
    let sign = |password: &str| {
        Command::new(tool)
            .args(["sign", file.to_str().unwrap(), "v0.3.0"])
            .env(
                "HAKU_SIGNING_KEY",
                std::fs::read_to_string(&secret).unwrap(),
            )
            .env("HAKU_SIGNING_PASSWORD", password)
            .status()
            .unwrap()
            .success()
    };
    assert!(!sign("wrong password"));
    assert!(sign("test password"));
    let sig = std::fs::read_to_string(dir.path().join(format!("{name}.minisig"))).unwrap();
    let pk = std::fs::read_to_string(&public).unwrap();
    assert_eq!(
        haku_core::update::verify(b"pretend installer", &sig, &pk, "0.3.0", &name),
        Ok(())
    );
    assert!(haku_core::update::verify(b"changed", &sig, &pk, "0.3.0", &name).is_err());
}
