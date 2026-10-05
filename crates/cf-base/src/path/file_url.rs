//! A path as a file URL (`pathToFileURL(path, { windows }).href`,
//! `lib/internal/url.js` of Node v26.8.1), for an absolute path, by either
//! platform's rules: OpenCode is handed its plugin by such a URL.

use super::{posix, win32};

/// `path` as the file URL Node writes for it, by Windows' rules or by
/// POSIX's; none for a path that is not absolute, which Node would have
/// resolved against the working folder.
///
/// The path is normalized as `path.resolve` normalizes an absolute one, a
/// final separator kept (Node adds it back). A Windows path names its drive
/// (`file:///C:/…`) or its server (`\\server\share` as `file://server/…`).
/// In a name, letters, digits and `!$&'()*+,-.:;=@_` are kept, and every
/// other character is written as its UTF-8 bytes, `%` and two capitals each.
/// A root alone (`/`, `C:\`) is not Node's to compare: Node adds a slash to
/// it by the separator of the system it runs on.
pub fn to_file_url(path: &str, windows: bool) -> Option<String> {
    if !windows {
        return path
            .starts_with('/')
            .then(|| format!("file://{}", encoded(&posix::normalize(path))));
    }
    if let Some(unc) = path.strip_prefix("\\\\") {
        let (host, rest) = unc.split_at(unc.find('\\')?);
        if host.is_empty() {
            return None;
        }
        return Some(format!(
            "file://{host}{}",
            encoded(&rest.replace('\\', "/"))
        ));
    }
    let bytes = path.as_bytes();
    let drive = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    drive.then(|| {
        let normalized = win32::normalize(path).replace('\\', "/");
        format!("file:///{}", encoded(&normalized))
    })
}

/// `path` with every character a file URL does not keep as it is written as
/// its UTF-8 bytes, `%XX` each.
fn encoded(path: &str) -> String {
    let mut written = String::with_capacity(path.len());
    for character in path.chars() {
        if character.is_ascii_alphanumeric() || "!$&'()*+,-.:;=@_/".contains(character) {
            written.push(character);
        } else {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).bytes() {
                written.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_path_is_a_file_url_and_a_relative_one_none() {
        assert_eq!(
            to_file_url("/a b/c#d", false).as_deref(),
            Some("file:///a%20b/c%23d")
        );
        assert_eq!(
            to_file_url("C:\\a b\\c", true).as_deref(),
            Some("file:///C:/a%20b/c")
        );
        assert_eq!(
            to_file_url("\\\\server\\share\\x", true).as_deref(),
            Some("file://server/share/x")
        );
        for (path, windows) in [
            ("a/b", false),
            ("a\\b", true),
            ("C:a", true),
            ("\\\\\\x", true),
        ] {
            assert_eq!(to_file_url(path, windows), None, "{path:?}");
        }
    }
}
