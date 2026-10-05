//! A path as a file URL (`pathToFileURL(path, { windows }).href`,
//! `lib/internal/url.js` and `PathToFileURL`, `src/node_url.cc`, of Node
//! v26.8.1), for an absolute path, by either platform's rules: OpenCode is
//! handed its plugin by such a URL. Node writes the path after `file://`
//! as its table says (`EncodePathChars`) and parses that as a URL by the
//! WHATWG standard (ada), whose parser here is the `url` crate's; a
//! server's name (`\\server\share`) is then set as the URL's host, as the
//! standard's setter sets a file URL's.

use std::path::MAIN_SEPARATOR;

use url::{Host, Url};

use super::{posix, win32};

/// `path` as the file URL Node writes for it, by Windows' rules or by
/// POSIX's; none where Node throws, and for a path that is not absolute,
/// which Node would have resolved against the working folder.
pub fn to_file_url(path: &str, windows: bool) -> Option<String> {
    let unc = windows && path.starts_with(r"\\");
    let resolved = if unc {
        path.to_owned()
    } else {
        resolved(path, windows)?
    };
    if windows && resolved.starts_with(r"\\") {
        // `\\server\share\…`, or `\\?\UNC\server\share\…`.
        let prefix = if resolved.starts_with(r"\\?\UNC\") {
            8
        } else {
            2
        };
        let end = prefix + resolved.get(prefix..)?.find('\\')?;
        if end == 2 {
            return None;
        }
        let mut url = Url::parse(&encoded(&resolved[end..], windows)).ok()?;
        set_host(&mut url, &resolved[prefix..end])?;
        return Some(url.into());
    }
    let mut resolved = resolved;
    // `path.resolve` took a final separator off, and Node puts one back, by
    // the separator of the system it runs on.
    let last = path.chars().last();
    if (last == Some('/') || (windows && last == Some('\\'))) && !resolved.ends_with(MAIN_SEPARATOR)
    {
        resolved.push('/');
    }
    Url::parse(&encoded(&resolved, windows))
        .ok()
        .map(Into::into)
}

/// `path.resolve` of an absolute path: normalized, with no final separator
/// but a root's. None for a path the working folder would make whole.
fn resolved(path: &str, windows: bool) -> Option<String> {
    if !windows {
        if !path.starts_with('/') {
            return None;
        }
        let normalized = posix::normalize(path);
        return Some(match normalized.strip_suffix('/') {
            Some(rest) if !rest.is_empty() => rest.to_owned(),
            _ => normalized,
        });
    }
    let normalized = win32::normalize(path);
    let root = win32_root(&normalized)?;
    Some(match normalized.strip_suffix('\\') {
        Some(rest) if rest.len() >= root => rest.to_owned(),
        _ => normalized,
    })
}

/// How long the root of a normalized Windows path is: `C:\`, or
/// `\\server\share\`; none for a path with no drive or server, which only
/// the working folder makes whole.
fn win32_root(normalized: &str) -> Option<usize> {
    let bytes = normalized.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        return Some(3);
    }
    let server = normalized.strip_prefix(r"\\")?;
    let share = server.find('\\')? + 1;
    let after = server[share..]
        .find('\\')
        .map_or(server.len(), |end| share + end + 1);
    Some(2 + after)
}

/// The URL Node parses for `path` (`EncodePathChars`): `file://`, then the
/// path, a Windows path's separators as `/`, and the characters RFC 1738
/// calls unsafe as `%XX`. What is past ASCII the parser writes as UTF-8.
fn encoded(path: &str, windows: bool) -> String {
    let mut written = String::from("file://");
    for character in path.chars() {
        match character {
            '\\' if windows => written.push('/'),
            '\0' | '\t' | '\n' | '\r' | ' ' | '"' | '#' | '%' | '?' | '[' | '\\' | ']' | '^'
            | '|' | '~' => written.push_str(&format!("%{:02X}", u32::from(character))),
            other => written.push(other),
        }
    }
    written
}

/// Sets `name` as a file URL's host, as the standard's setter does
/// (`set_hostname`): up to its first `/`, `\`, `?` or `#`; none for
/// nothing or `localhost`; else the host it parses as, a name made ASCII.
/// None for one that is no host.
fn set_host(url: &mut Url, name: &str) -> Option<()> {
    let name = name.split(['/', '\\', '?', '#']).next().unwrap_or_default();
    if name.is_empty() {
        return url.set_host(None).ok();
    }
    let host = Host::parse(name).ok()?;
    if matches!(&host, Host::Domain(domain) if domain == "localhost") {
        return url.set_host(None).ok();
    }
    url.set_host(Some(&host.to_string())).ok()
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
            ("\\x", true),
            ("\\\\\\x", true),
        ] {
            assert_eq!(to_file_url(path, windows), None, "{path:?}");
        }
    }
}
