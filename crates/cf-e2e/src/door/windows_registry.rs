//! The names of the variables the account's registry stores (T-2086 B1): what portable-pty's
//! builder puts over the pane host's own environment in each window it starts on Windows, from
//! the machine's `HKLM\System\CurrentControlSet\Control\Session Manager\Environment` and the
//! user's `HKCU\Environment`. Only the names are read; no value is asked for. Nothing is written.
//!
//! A read that fails is a refusal, not "no names" (T-2119 B1-a): the builder goes on past a value
//! it cannot read, so a window sent after a failed read could have a variable the seal never
//! judged. The one answer that is no failure is a key that is not there.
#![cfg_attr(not(windows), allow(dead_code))] // The OS calls, and so their callers, are Windows'.

/// `ERROR_SUCCESS`.
const SUCCESS: u32 = 0;
/// `ERROR_FILE_NOT_FOUND`: what opening a key that is not there says.
const NO_SUCH_KEY: u32 = 2;
/// `ERROR_NO_MORE_ITEMS`: what listing a key's values says at their end.
const NO_MORE_VALUES: u32 = 259;

/// What opening the key `path` said (`code`, a Win32 error): whether it is open. A key that is
/// not there holds no names; any other answer is the error.
fn opened(code: u32, path: &str) -> Result<bool, String> {
    match code {
        SUCCESS => Ok(true),
        NO_SUCH_KEY => Ok(false),
        other => Err(format!(
            "the registry key {path} cannot be opened (error {other}), so the variables it stores are not known"
        )),
    }
}

/// What listing the values of `path` said at one of them (`code`): whether there is a name. The
/// end of the values is no error; any other answer is.
fn listed(code: u32, path: &str) -> Result<bool, String> {
    match code {
        SUCCESS => Ok(true),
        NO_MORE_VALUES => Ok(false),
        other => Err(format!(
            "the registry key {path} cannot be listed to its end (error {other}), so the variables it stores are not known"
        )),
    }
}

#[cfg(windows)]
pub use os::stored_names;

#[cfg(windows)]
mod os {
    #![allow(unsafe_code)] // The registry's functions, which std does not give.

    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};

    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_QUERY_VALUE,
    };

    /// The longest name of a registry value, in UTF-16 units, with its end.
    const LONGEST: usize = 16_384;

    /// Every name the two keys hold, the machine's first, as the builder reads them; the error
    /// where either cannot be read.
    pub fn stored_names() -> Result<Vec<OsString>, String> {
        let mut names = names_in(
            HKEY_LOCAL_MACHINE,
            r"System\CurrentControlSet\Control\Session Manager\Environment",
        )?;
        names.extend(names_in(HKEY_CURRENT_USER, "Environment")?);
        Ok(names)
    }

    /// The names of the values of `path` below `root`; none where the key is not there.
    fn names_in(root: HKEY, path: &str) -> Result<Vec<OsString>, String> {
        let wide: Vec<u16> = std::ffi::OsStr::new(path)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: the path is a string ended by 0, and `key` is ours to fill.
        let code = unsafe { RegOpenKeyExW(root, wide.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) };
        if !super::opened(code, path)? {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        let mut name = vec![0u16; LONGEST];
        let mut failed = None;
        for index in 0u32.. {
            let mut length = u32::try_from(name.len()).unwrap_or(u32::MAX);
            // SAFETY: `key` is open, the name's buffer is `length` units long, and no type or
            // data is asked for.
            let code = unsafe {
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
            match super::listed(code, path) {
                Ok(true) => {
                    let length = usize::try_from(length).unwrap_or(0).min(name.len());
                    names.push(OsString::from_wide(&name[..length]));
                }
                Ok(false) => break,
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            }
        }
        // SAFETY: `key` was opened above and is closed once.
        unsafe { RegCloseKey(key) };
        match failed {
            Some(error) => Err(error),
            None => Ok(names),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-2119 B1-a: only a key that is not there holds no names; a key that cannot be opened
    /// (access denied, 5) or a listing that stops on an error (more data, 234) is a refusal.
    #[test]
    fn a_registry_read_fails_closed_on_every_error_but_no_such_key() {
        assert_eq!(opened(0, "k"), Ok(true));
        assert_eq!(opened(2, "k"), Ok(false));
        for code in [1, 3, 5, 6, 87, 1018] {
            let refused = opened(code, "Environment").unwrap_err();
            assert!(refused.contains("Environment"), "{refused}");
            assert!(refused.contains(&format!("error {code}")), "{refused}");
        }
        assert_eq!(listed(0, "k"), Ok(true));
        assert_eq!(listed(259, "k"), Ok(false));
        for code in [2, 5, 234, 1018] {
            assert!(listed(code, "k").is_err(), "{code}");
        }
    }
}
