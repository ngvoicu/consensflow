//! `path.posix`: `normalize` and `join`, as Node's POSIX flavour has them.

use super::{code_at, is_posix_path_separator, normalize_string};

/// `path.posix.normalize`.
pub fn normalize(path: &str) -> String {
    let len = path.len();
    if len == 0 {
        return ".".to_owned();
    }

    let is_absolute = code_at(path, 0) == Some(b'/');
    // The last unit is a `/` when the last byte is: no other character has a
    // byte of 0x2F.
    let trailing_separator = code_at(path, len - 1) == Some(b'/');

    // Normalize the path
    let mut path = normalize_string(path, !is_absolute, '/', is_posix_path_separator);

    if path.is_empty() {
        if is_absolute {
            return "/".to_owned();
        }
        return if trailing_separator { "./" } else { "." }.to_owned();
    }
    if trailing_separator {
        path.push('/');
    }

    if is_absolute {
        format!("/{path}")
    } else {
        path
    }
}

/// `path.posix.join`.
pub fn join(parts: &[&str]) -> String {
    if parts.is_empty() {
        return ".".to_owned();
    }

    let path: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|part| !part.is_empty())
        .collect();

    if path.is_empty() {
        return ".".to_owned();
    }

    normalize(&path.join("/"))
}
