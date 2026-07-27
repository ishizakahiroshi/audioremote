//! Minimal per-user Windows logon autostart registration.
//!
//! Only the application's own HKCU Run value is touched. Firewall rules,
//! elevation, task-tray integration, and background process management belong
//! to the v0.2 autostart work.

use std::io;
use std::path::Path;

pub const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "AudioRemote";

/// Build the exact command stored in the Run value.
pub fn command_value(exe: &Path) -> String {
    format!("\"{}\" serve --no-open", exe.display())
}

#[cfg(windows)]
fn registry_error(code: windows::Win32::Foundation::WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(code.0 as i32)
}

#[cfg(windows)]
pub fn install(exe: &Path) -> io::Result<String> {
    use std::slice;

    use windows::core::w;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyW, RegSetValueExW, HKEY_CURRENT_USER, REG_SZ,
    };

    if !exe.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "current executable path is not absolute",
        ));
    }

    let value = command_value(exe);
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = unsafe { slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };

    let mut key = HKEY_CURRENT_USER;
    let status = unsafe {
        RegCreateKeyW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(registry_error(status));
    }

    let status = unsafe { RegSetValueExW(key, w!("AudioRemote"), 0, REG_SZ, Some(bytes)) };
    let close_status = unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS {
        return Err(registry_error(status));
    }
    if close_status != ERROR_SUCCESS {
        return Err(registry_error(close_status));
    }
    Ok(value)
}

#[cfg(windows)]
pub fn uninstall() -> io::Result<()> {
    use windows::core::w;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
    };

    let mut key = HKEY_CURRENT_USER;
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if status != ERROR_SUCCESS {
        return Err(registry_error(status));
    }

    let status = unsafe { RegDeleteValueW(key, w!("AudioRemote")) };
    let close_status = unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        return Err(registry_error(status));
    }
    if close_status != ERROR_SUCCESS {
        return Err(registry_error(close_status));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn install(_exe: &Path) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "autostart is supported on Windows only",
    ))
}

#[cfg(not(windows))]
pub fn uninstall() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "autostart is supported on Windows only",
    ))
}

#[cfg(test)]
mod tests {
    use super::command_value;
    use std::path::Path;

    #[test]
    fn command_quotes_paths_with_spaces_and_unicode() {
        let path = Path::new(r"C:\Tools\音声 Remote\audioremote.exe");
        assert_eq!(
            command_value(path),
            r#""C:\Tools\音声 Remote\audioremote.exe" serve --no-open"#
        );
    }

    #[test]
    fn command_keeps_argument_boundary() {
        assert_eq!(
            command_value(Path::new(r"C:\audioremote.exe")),
            r#""C:\audioremote.exe" serve --no-open"#
        );
    }
}
