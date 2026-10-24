/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Operating-system protection for the password vault and sync keys: Windows DPAPI wraps keys
//! so only this Windows user can unwrap them, and Windows Hello confirms it is really the user
//! before passwords are shown, exported or filled into payment forms.

use haku_core::vault::Protector;
use zeroize::Zeroizing;

/// DPAPI in the current user's scope, with an application-specific entropy value so other
/// programs running as the same user cannot unwrap HakuCX's blobs by simply calling DPAPI.
pub struct Dpapi;

#[cfg(windows)]
const ENTROPY: &[u8] = b"HakuCX vault key v1";

#[cfg(windows)]
impl Protector for Dpapi {
    fn protect(&self, secret: &[u8]) -> Result<Vec<u8>, String> {
        use windows::Win32::Foundation::{HLOCAL, LocalFree};
        use windows::Win32::Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
        };
        let input = CRYPT_INTEGER_BLOB {
            cbData: secret.len() as u32,
            pbData: secret.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: the input blobs point at live slices for the duration of the call; DPAPI
        // allocates `out` with LocalAlloc, which we copy and free.
        unsafe {
            CryptProtectData(
                &input,
                windows::core::w!("HakuCX"),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|e| e.message())?;
            let blob = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(Some(HLOCAL(out.pbData as _)));
            Ok(blob)
        }
    }

    fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        use windows::Win32::Foundation::{HLOCAL, LocalFree};
        use windows::Win32::Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
        };
        let input = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`; the plaintext is wiped before the buffer is freed.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .map_err(|e| e.message())?;
            let plain = std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize);
            let copy = Zeroizing::new(plain.to_vec());
            zeroize::Zeroize::zeroize(plain);
            LocalFree(Some(HLOCAL(out.pbData as _)));
            Ok(copy)
        }
    }
}

#[cfg(not(windows))]
impl Protector for Dpapi {
    fn protect(&self, _secret: &[u8]) -> Result<Vec<u8>, String> {
        Err("no operating-system key store on this platform yet".into())
    }

    fn unprotect(&self, _blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        Err("no operating-system key store on this platform yet".into())
    }
}

/// Is Windows Hello (face, fingerprint or PIN) set up for this user?
#[cfg(windows)]
pub fn hello_available() -> bool {
    use windows::Security::Credentials::UI::{
        UserConsentVerifier, UserConsentVerifierAvailability,
    };
    UserConsentVerifier::CheckAvailabilityAsync()
        .and_then(|op| op.join())
        .is_ok_and(|a| a == UserConsentVerifierAvailability::Available)
}

#[cfg(not(windows))]
pub fn hello_available() -> bool {
    false
}

/// Ask Windows Hello to verify the user, with the dialog owned by `hwnd`. Blocks until the
/// user answers: call it from a worker thread.
#[cfg(windows)]
pub fn hello_verify(hwnd: isize, message: &str) -> Result<bool, String> {
    use windows::Security::Credentials::UI::{UserConsentVerificationResult, UserConsentVerifier};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
    use windows_future::IAsyncOperation;
    let interop = windows::core::factory::<UserConsentVerifier, IUserConsentVerifierInterop>()
        .map_err(|e| e.message())?;
    // SAFETY: `hwnd` is the browser's top-level window, alive while the browser runs.
    let op: IAsyncOperation<UserConsentVerificationResult> = unsafe {
        interop.RequestVerificationForWindowAsync(
            HWND(hwnd as *mut core::ffi::c_void),
            &windows::core::HSTRING::from(message),
        )
    }
    .map_err(|e| e.message())?;
    let result = op.join().map_err(|e| e.message())?;
    Ok(result == UserConsentVerificationResult::Verified)
}

#[cfg(not(windows))]
pub fn hello_verify(_hwnd: isize, _message: &str) -> Result<bool, String> {
    Err("Windows Hello is only available on Windows".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn dpapi_round_trip() {
        let blob = Dpapi.protect(b"0123456789abcdef0123456789abcdef").unwrap();
        assert_ne!(blob.as_slice(), b"0123456789abcdef0123456789abcdef");
        assert_eq!(
            Dpapi.unprotect(&blob).unwrap().as_slice(),
            b"0123456789abcdef0123456789abcdef"
        );
        let mut damaged = blob.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        assert!(Dpapi.unprotect(&damaged).is_err());
    }
}
