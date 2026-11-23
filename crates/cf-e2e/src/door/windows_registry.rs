//! The names of the variables the account's registry stores (T-2086 B1): what portable-pty's
//! builder puts over the pane host's own environment in each window it starts on Windows, from
//! the machine's `HKLM\System\CurrentControlSet\Control\Session Manager\Environment` and the
//! user's `HKCU\Environment`. Only the names are read; no value is asked for. Nothing is written.
#![allow(unsafe_code)] // The registry's functions, which std does not give.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    KEY_QUERY_VALUE,
};

/// The longest name of a registry value, in UTF-16 units, with its end.
const LONGEST: usize = 16_384;

/// Every name the two keys hold, the machine's first, as the builder reads them.
pub fn stored_names() -> Vec<OsString> {
    let mut names = names_in(
        HKEY_LOCAL_MACHINE,
        r"System\CurrentControlSet\Control\Session Manager\Environment",
    );
    names.extend(names_in(HKEY_CURRENT_USER, "Environment"));
    names
}

/// The names of the values of `path` below `root`; none where it cannot be opened.
fn names_in(root: HKEY, path: &str) -> Vec<OsString> {
    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: the path is a string ended by 0, and `key` is ours to fill.
    if unsafe { RegOpenKeyExW(root, wide.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut names = Vec::new();
    let mut name = vec![0u16; LONGEST];
    for index in 0u32.. {
        let mut length = u32::try_from(name.len()).unwrap_or(u32::MAX);
        // SAFETY: `key` is open, the name's buffer is `length` units long, and no type or data is
        // asked for.
        let read = unsafe {
            RegEnumValueW(
                key,
                index,
                name.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        // The end of the values (`ERROR_NO_MORE_ITEMS`), or a key that cannot be read further.
        if read != ERROR_SUCCESS {
            break;
        }
        let length = usize::try_from(length).unwrap_or(0).min(name.len());
        names.push(OsString::from_wide(&name[..length]));
    }
    // SAFETY: `key` was opened above and is closed once.
    unsafe { RegCloseKey(key) };
    names
}
