/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Windows integration, all per user (`HKEY_CURRENT_USER`, no administrator rights): listing
//! HakuCX as a browser Windows can make the default, starting HakuCX when you sign in, and
//! telling an installed copy from a portable one.
//!
//! Windows does not let programs make themselves the default browser (the choice is protected
//! by a hash only Windows can compute). HakuCX registers itself the way every browser does and
//! then opens Windows Settings at its own entry, where you confirm the choice.

use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "HakuCX";
const PROGID_URL: &str = "HakuCXURL";
const PROGID_HTML: &str = "HakuCXHTML";
const CLIENT_KEY: &str = r"Software\Clients\StartMenuInternet\HakuCX";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\HakuCX";
/// Documents HakuCX can open (Servo has no PDF viewer, so not `.pdf`).
const FILE_TYPES: &[&str] = &[".htm", ".html", ".shtml", ".xht", ".xhtml", ".svg", ".webp"];
const URL_SCHEMES: &[&str] = &["http", "https"];

pub fn current_exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// How Windows starts HakuCX for a link or file. `--` ends option parsing, so a link that
/// starts with `--` can never pass HakuCX an option.
pub fn open_command(exe: &Path) -> String {
    format!("\"{}\" -- \"%1\"", exe.display())
}

#[cfg(windows)]
mod reg {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW,
        RegSetValueExW,
    };
    use windows::core::{HSTRING, PCWSTR};

    fn err(code: windows::Win32::Foundation::WIN32_ERROR) -> std::io::Error {
        std::io::Error::from_raw_os_error(code.0 as i32)
    }

    /// Set a string value (`name` empty = the key's default value), creating the key.
    pub fn set(path: &str, name: &str, value: &str) -> std::io::Result<()> {
        let mut key = HKEY::default();
        // SAFETY: plain registry calls with owned, NUL-terminated strings; the key is closed.
        unsafe {
            let r = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            );
            if r != ERROR_SUCCESS {
                return Err(err(r));
            }
            let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
            let name = HSTRING::from(name);
            let r = RegSetValueExW(
                key,
                if name.is_empty() {
                    PCWSTR::null()
                } else {
                    PCWSTR(name.as_ptr())
                },
                None,
                REG_SZ,
                Some(bytes),
            );
            let _ = RegCloseKey(key);
            if r != ERROR_SUCCESS {
                return Err(err(r));
            }
        }
        Ok(())
    }

    pub fn get(path: &str, name: &str) -> Option<String> {
        let name = HSTRING::from(name);
        let name_ptr = if name.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(name.as_ptr())
        };
        let mut size: u32 = 0;
        // SAFETY: first call asks for the size, second fills a buffer of that size.
        unsafe {
            if RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                name_ptr,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let mut buf = vec![0u16; (size as usize).div_ceil(2)];
            if RegGetValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                name_ptr,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut size),
            ) != ERROR_SUCCESS
            {
                return None;
            }
            let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }

    pub fn delete_tree(path: &str) -> std::io::Result<()> {
        // SAFETY: deletes a key under HKCU by path.
        let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path)) };
        if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(err(r))
        }
    }

    pub fn delete_value(path: &str, name: &str) -> std::io::Result<()> {
        // SAFETY: deletes one value under HKCU by path and name.
        let r = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(path),
                &HSTRING::from(name),
            )
        };
        if r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(err(r))
        }
    }

    /// Tell Explorer that file and link associations changed.
    pub fn notify_associations() {
        use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
        // SAFETY: no pointers are passed.
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    }
}

/// Register HakuCX (at `exe`) with Windows as a browser for web links and HTML files.
#[cfg(windows)]
pub fn register_browser(exe: &Path) -> std::io::Result<()> {
    let icon = format!("\"{}\",0", exe.display());
    let open = open_command(exe);
    for (progid, title) in [
        (PROGID_URL, "HakuCX URL"),
        (PROGID_HTML, "HakuCX HTML Document"),
    ] {
        let base = format!(r"Software\Classes\{progid}");
        reg::set(&base, "", title)?;
        reg::set(&base, "FriendlyTypeName", title)?;
        reg::set(&format!(r"{base}\DefaultIcon"), "", &icon)?;
        reg::set(&format!(r"{base}\shell\open\command"), "", &open)?;
        reg::set(&format!(r"{base}\Application"), "ApplicationName", APP_NAME)?;
        reg::set(&format!(r"{base}\Application"), "ApplicationIcon", &icon)?;
        reg::set(
            &format!(r"{base}\Application"),
            "ApplicationCompany",
            APP_NAME,
        )?;
    }
    reg::set(CLIENT_KEY, "", APP_NAME)?;
    reg::set(&format!(r"{CLIENT_KEY}\DefaultIcon"), "", &icon)?;
    reg::set(
        &format!(r"{CLIENT_KEY}\shell\open\command"),
        "",
        &format!("\"{}\"", exe.display()),
    )?;
    let caps = format!(r"{CLIENT_KEY}\Capabilities");
    reg::set(&caps, "ApplicationName", APP_NAME)?;
    reg::set(&caps, "ApplicationIcon", &icon)?;
    reg::set(
        &caps,
        "ApplicationDescription",
        "An independent GX-style browser built on Servo",
    )?;
    reg::set(&format!(r"{caps}\StartMenu"), "StartMenuInternet", APP_NAME)?;
    for ext in FILE_TYPES {
        reg::set(&format!(r"{caps}\FileAssociations"), ext, PROGID_HTML)?;
    }
    for scheme in URL_SCHEMES {
        reg::set(&format!(r"{caps}\URLAssociations"), scheme, PROGID_URL)?;
    }
    reg::set(r"Software\RegisteredApplications", APP_NAME, &caps)?;
    reg::notify_associations();
    Ok(())
}

/// Remove everything [`register_browser`] wrote.
#[cfg(windows)]
pub fn unregister_browser() -> std::io::Result<()> {
    reg::delete_tree(&format!(r"Software\Classes\{PROGID_URL}"))?;
    reg::delete_tree(&format!(r"Software\Classes\{PROGID_HTML}"))?;
    reg::delete_tree(CLIENT_KEY)?;
    reg::delete_value(r"Software\RegisteredApplications", APP_NAME)?;
    reg::notify_associations();
    Ok(())
}

/// Is this copy of HakuCX the one registered with Windows?
#[cfg(windows)]
pub fn is_registered(exe: &Path) -> bool {
    reg::get(
        &format!(r"Software\Classes\{PROGID_URL}\shell\open\command"),
        "",
    )
    .is_some_and(|c| c == open_command(exe))
}

/// Has the user chosen HakuCX for web links (https and http)?
#[cfg(windows)]
pub fn is_default() -> bool {
    URL_SCHEMES.iter().all(|scheme| {
        reg::get(
            &format!(
                r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice"
            ),
            "ProgId",
        )
        .is_some_and(|p| p == PROGID_URL)
    })
}

/// Open Windows Settings at HakuCX's default-apps page (Windows 11), or the default apps page.
#[cfg(windows)]
pub fn open_default_apps() {
    let _ = open::that_detached(format!(
        "ms-settings:defaultapps?registeredAppUser={APP_NAME}"
    ));
}

/// Start HakuCX when the user signs in to Windows (or stop).
#[cfg(windows)]
pub fn set_run_at_login(exe: &Path, on: bool) -> std::io::Result<()> {
    if on {
        reg::set(RUN_KEY, APP_NAME, &format!("\"{}\"", exe.display()))
    } else {
        reg::delete_value(RUN_KEY, APP_NAME)
    }
}

/// The command Windows runs at sign-in for HakuCX, if any.
#[cfg(windows)]
pub fn run_at_login_command() -> Option<String> {
    reg::get(RUN_KEY, APP_NAME)
}

/// Was this copy installed by the HakuCX installer (rather than unpacked from the portable
/// zip or built from source)?
#[cfg(windows)]
pub fn installed_dir() -> Option<PathBuf> {
    reg::get(UNINSTALL_KEY, "InstallLocation").map(PathBuf::from)
}

#[cfg(not(windows))]
mod unsupported {
    use super::*;
    pub fn register_browser(_exe: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("only on Windows"))
    }
    pub fn unregister_browser() -> std::io::Result<()> {
        Ok(())
    }
    pub fn is_registered(_exe: &Path) -> bool {
        false
    }
    pub fn is_default() -> bool {
        false
    }
    pub fn open_default_apps() {}
    pub fn set_run_at_login(_exe: &Path, _on: bool) -> std::io::Result<()> {
        Err(std::io::Error::other("only on Windows"))
    }
    pub fn run_at_login_command() -> Option<String> {
        None
    }
    pub fn installed_dir() -> Option<PathBuf> {
        None
    }
}
#[cfg(not(windows))]
pub use unsupported::*;

/// Is the running copy the installed one?
pub fn is_installed_copy() -> bool {
    match (
        installed_dir(),
        current_exe().and_then(|e| e.parent().map(Path::to_path_buf)),
    ) {
        (Some(installed), Some(here)) => same_path(&installed, &here),
        _ => false,
    }
}

/// A portable copy keeps its data next to itself: `portable` (a marker file shipped in the
/// portable zip) beside `hakucx.exe` means "store profiles in `Data` here".
pub fn portable_data_dir() -> Option<PathBuf> {
    let dir = current_exe()?.parent()?.to_path_buf();
    dir.join("portable").is_file().then(|| dir.join("Data"))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_command_quotes_and_ends_options() {
        assert_eq!(
            open_command(Path::new(r"C:\Apps\HakuCX\hakucx.exe")),
            r#""C:\Apps\HakuCX\hakucx.exe" -- "%1""#
        );
        assert!(same_path(Path::new(r"C:\A\B\"), Path::new(r"c:\a\b")));
    }

    #[cfg(windows)]
    #[test]
    fn registry_round_trip() {
        let path = r"Software\HakuCX-test-registry";
        reg::set(path, "", "default").unwrap();
        reg::set(path, "Name", "välue").unwrap();
        assert_eq!(reg::get(path, "").as_deref(), Some("default"));
        assert_eq!(reg::get(path, "Name").as_deref(), Some("välue"));
        reg::delete_value(path, "Name").unwrap();
        assert_eq!(reg::get(path, "Name"), None);
        reg::delete_tree(path).unwrap();
        assert_eq!(reg::get(path, ""), None);
    }
}
