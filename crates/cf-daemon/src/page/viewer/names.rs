//! The names of a path in a specs folder, as the page writes one: relative, in
//! slashes, each part a plain name, refused before anything is opened where a
//! system would read it as something other than a name of the folder.

/// The names `path` is made of, as the page writes a path of the folder:
/// relative, in slashes. Refused where it is empty, absolute, or climbs (`..`),
/// where a name is empty or `.`, where it holds a NUL, and on Windows
/// (`windows`) where a name is read as something else than it says: one that
/// holds a backslash or a colon, a path of its own (`\\server`, `C:`, a file's
/// stream), one that ends in a dot or a space, which Windows drops, and one
/// Windows keeps for a device in every folder (`CON`, `nul.md`).
pub(super) fn parts(path: &str, windows: bool) -> Result<Vec<&str>, String> {
    if path.is_empty() {
        return Err("a path in the specs folder names a file: this one is empty".to_owned());
    }
    if path.starts_with('/') || (windows && path.starts_with('\\')) {
        return Err(format!(
            "{path} is absolute: a path in the specs folder is relative to it"
        ));
    }
    if path.contains('\0') {
        return Err(format!("{path} holds a NUL"));
    }
    let named: Vec<&str> = path.split('/').collect();
    for part in &named {
        if *part == ".." {
            return Err(format!("{path} climbs out of the specs folder (..)"));
        }
        if part.is_empty() || *part == "." {
            return Err(format!(
                "{path} is no plain path: a name in it is empty or ."
            ));
        }
        if windows && part.contains('\\') {
            return Err(format!(
                "{path} holds a backslash: a path in the specs folder is written in slashes"
            ));
        }
        if windows && part.contains(':') {
            return Err(format!("{path} names a drive or a stream (:)"));
        }
        if windows && (part.ends_with('.') || part.ends_with(' ')) {
            return Err(format!(
                "{path} has a name ending in a dot or a space, which Windows reads without it"
            ));
        }
        if windows && is_device(part) {
            return Err(format!("{path} names a device of Windows ({part})"));
        }
    }
    Ok(named)
}

/// Whether `name` may be, on Windows (`windows`), the short (8.3) name of
/// another file, which Windows answers to as it answers to the long one: a `~`
/// and a digit in its base name (`GIT~1` is `.git` where the volume keeps short
/// names). The long name may be a dotfile, which the viewer leaves out, so a
/// name that may be an alias is left out too, as the page writes it, without
/// asking the volume; one that only looks like one (`notes~2.md`) is left out
/// with them.
pub(super) fn is_short_alias(name: &str, windows: bool) -> bool {
    let base = name.rsplit_once('.').map_or(name, |(base, _)| base);
    windows
        && base
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b'~' && pair[1].is_ascii_digit())
}

/// Whether `name` is one Windows keeps for a device in every folder (`CON`,
/// `nul.md`, `COM1`), whatever its case and its extension.
fn is_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let numbered = |prefix: &[u8]| match stem.as_bytes() {
        [a, b, c, digit] => {
            [*a, *b, *c].eq_ignore_ascii_case(prefix) && matches!(digit, b'1'..=b'9')
        }
        _ => false,
    };
    ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|device| stem.eq_ignore_ascii_case(device))
        || numbered(b"COM")
        || numbered(b"LPT")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of a path, as each system reads one: on Windows a backslash and a
    /// colon are a path of their own, a name's last dots and spaces are dropped and
    /// a device's name is the device's in every folder, so a name is none of these.
    #[test]
    fn a_path_is_names_in_slashes_on_either_system() {
        let read = |path, windows| parts(path, windows).map_err(|_| ());
        for windows in [false, true] {
            assert_eq!(read("a/b.md", windows), Ok(vec!["a", "b.md"]));
            assert_eq!(read("registry.md", windows), Ok(vec!["registry.md"]));
            assert_eq!(
                read(".specs", windows),
                Ok(vec![".specs"]),
                "the folder may be a dotfolder"
            );
            for refused in [
                "", "/a", "a//b", "a/./b", "./a", "a/", "a/../b", "..", "a\0b",
            ] {
                assert_eq!(
                    read(refused, windows),
                    Err(()),
                    "{refused:?} on windows: {windows}"
                );
            }
        }
        for path in [
            r"\a",
            r"a\b",
            r"..\a",
            r"\\server\share",
            "C:/a",
            "C:a",
            "a/b.md:secret",
            r"C:\a",
            "a/SPEC.md.",
            "a /SPEC.md",
            ".. /x",
            "CON",
            "a/nul.md",
            "a/Com1.txt",
            "lpt9",
            "AUX .md",
        ] {
            assert_eq!(read(path, true), Err(()), "{path:?} on Windows");
        }
        // Elsewhere each is only a name; on Windows too, a name that only starts like a device's.
        for path in [r"a\b", "a/SPEC.md.", "CON", "a/nul.md", "a/Com1.txt"] {
            assert!(read(path, false).is_ok(), "{path:?} elsewhere");
        }
        for name in [
            "console.md",
            "COM10",
            "COM0",
            "lpt",
            "nullable.md",
            "aux-notes.md",
            "Cóm1",
        ] {
            assert_eq!(read(name, true), Ok(vec![name]), "{name:?} on Windows");
        }
        assert_eq!(read("C:/a", false), Ok(vec!["C:", "a"]));
        assert_eq!(
            parts(r"a\..\b", true).unwrap_err(),
            r"a\..\b holds a backslash: a path in the specs folder is written in slashes"
        );
        assert_eq!(
            parts("C:/a", true).unwrap_err(),
            "C:/a names a drive or a stream (:)"
        );
        assert_eq!(
            parts("a/nul.md", true).unwrap_err(),
            "a/nul.md names a device of Windows (nul.md)"
        );
        assert_eq!(
            parts("a/SPEC.md.", true).unwrap_err(),
            "a/SPEC.md. has a name ending in a dot or a space, which Windows reads without it"
        );
    }

    /// A short (8.3) name, which Windows answers to for a long one, a dotfile's
    /// among them, by its `~` and digit; only on Windows.
    #[test]
    fn a_short_name_is_one_with_a_tilde_and_a_digit_on_windows() {
        for name in [
            "GIT~1",
            "git~12",
            "PROGRA~1.TXT",
            "notes~2.md",
            "~1",
            "AB12~1.HTM",
        ] {
            assert!(is_short_alias(name, true), "{name:?} on Windows");
            assert!(!is_short_alias(name, false), "{name:?} elsewhere");
        }
        for name in [
            "SPEC.md",
            "a~b.md",
            "notes~.md",
            "~draft.md",
            "x.~1",
            ".git",
        ] {
            assert!(!is_short_alias(name, true), "{name:?} on Windows");
        }
    }
}
