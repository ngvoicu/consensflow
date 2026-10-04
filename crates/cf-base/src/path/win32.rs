//! `path.win32`: `normalize` and `join`, as Node's Windows flavour has them,
//! with its rules for Windows' reserved device names and CVE-2024-36139.

use super::{code_at, is_path_separator, normalize_string, slice};
use crate::text::utf16_len;

/// The names Windows keeps for devices. The last six end in the superscript
/// digits one, two and three, which `path.js` writes `\xb9`, `\xb2` and `\xb3`.
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "COM¹", "COM²",
    "COM³", "LPT¹", "LPT²", "LPT³",
];

/// Whether the text of `path` before `colon_index` is a reserved name, in any
/// case. A `colon_index` of none is `indexOf(':')` finding no colon, and
/// JavaScript's `slice(0, -1)` then takes all of `path` but its last UTF-16
/// unit. When that unit is half of a character outside the BMP, what is left
/// ends in half of one: no name does, and Rust text cannot hold it.
fn is_windows_reserved_name(path: &str, colon_index: Option<usize>) -> bool {
    let device = match colon_index {
        Some(end) => Some(slice(path, 0, end)),
        None => path.strip_suffix(|last: char| last.len_utf16() == 1),
    };
    // Unicode's upper case, as `toUpperCase` has it. No character outside ASCII
    // upper-cases to letters that all occur in the names (`ß` to `SS`, `ı` to
    // `I`), so only ASCII letters can spell one.
    device.is_some_and(|device| WINDOWS_RESERVED_NAMES.contains(&device.to_uppercase().as_str()))
}

fn is_windows_device_root(code: Option<u8>) -> bool {
    code.is_some_and(|code| code.is_ascii_alphabetic())
}

/// `path.win32.normalize`.
pub fn normalize(path: &str) -> String {
    let len = path.len();
    if len == 0 {
        return ".".to_owned();
    }
    let mut root_end = 0;
    let mut device: Option<String> = None;
    let mut is_absolute = false;
    let code = code_at(path, 0);

    // Try to match a root
    // `len === 1`, counted in UTF-16 units: `é` is one unit and two bytes.
    if utf16_len(path) == 1 {
        // `path` contains just a single char, exit early to avoid
        // unnecessary work
        return if code == Some(b'/') {
            "\\".to_owned()
        } else {
            path.to_owned()
        };
    }
    if is_path_separator(code) {
        // Possible UNC root

        // If we started with a separator, we know we at least have an absolute
        // path of some kind (UNC or otherwise)
        is_absolute = true;

        if is_path_separator(code_at(path, 1)) {
            // Matched double path separator at beginning. `j`, `last` and `len`
            // are bytes: the loops stop at an ASCII separator or at the end, and
            // skip any other character, byte by byte, as they would unit by unit.
            let mut j = 2;
            let mut last = j;
            // Match 1 or more non-path separators
            while j < len && !is_path_separator(code_at(path, j)) {
                j += 1;
            }
            if j < len && j != last {
                let first_part = slice(path, last, j);
                // Matched!
                last = j;
                // Match 1 or more path separators
                while j < len && is_path_separator(code_at(path, j)) {
                    j += 1;
                }
                if j < len && j != last {
                    // Matched!
                    last = j;
                    // Match 1 or more non-path separators
                    while j < len && !is_path_separator(code_at(path, j)) {
                        j += 1;
                    }
                    if j == len || j != last {
                        if first_part == "." || first_part == "?" {
                            // We matched a device root (e.g. \\.\PHYSICALDRIVE0)
                            device = Some(format!("\\\\{first_part}"));
                            root_end = 4;
                            let colon_index = path.find(':');
                            // Special case: handle \\?\COM1: or similar reserved device paths
                            let possible_device =
                                slice(path, 4, colon_index.map_or(0, |at| at + 1));
                            // `possibleDevice.length`, in bytes like `rootEnd`, which is
                            // compared with `len` and cuts `path`: `COM¹:` is five units
                            // and six bytes, and the root ends after its sixth byte.
                            if is_windows_reserved_name(
                                possible_device,
                                possible_device.len().checked_sub(1),
                            ) {
                                device = Some(format!("\\\\?\\{possible_device}"));
                                root_end = 4 + possible_device.len();
                            }
                        } else if j == len {
                            // We matched a UNC root only
                            // Return the normalized version of the UNC root since there
                            // is nothing left to process
                            return format!("\\\\{first_part}\\{}\\", slice(path, last, len));
                        } else {
                            // We matched a UNC root with leftovers
                            device = Some(format!("\\\\{first_part}\\{}", slice(path, last, j)));
                            root_end = j;
                        }
                    }
                }
            }
        } else {
            root_end = 1;
        }
    } else {
        let colon_index = path.find(':');
        if let Some(colon_index) = colon_index.filter(|&at| at > 0) {
            // `colonIndex > 0` is a colon after the first character, and `=== 1`
            // after a letter: that is one byte and one unit, so byte 1 and unit 1
            // are the same place.
            if is_windows_device_root(code) && colon_index == 1 {
                device = Some(slice(path, 0, 2).to_owned());
                root_end = 2;
                // `len > 2`: the two characters before are ASCII, so there is more
                // than two units exactly when there is more than two bytes.
                if len > 2 && is_path_separator(code_at(path, 2)) {
                    is_absolute = true;
                    root_end = 3;
                }
            } else if is_windows_reserved_name(path, Some(colon_index)) {
                device = Some(slice(path, 0, colon_index + 1).to_owned());
                root_end = colon_index + 1;
            }
        }
    }

    let mut tail = if root_end < len {
        normalize_string(
            slice(path, root_end, len),
            !is_absolute,
            '\\',
            is_path_separator,
        )
    } else {
        String::new()
    };
    if tail.is_empty() && !is_absolute {
        tail = ".".to_owned();
    }
    if !tail.is_empty() && is_path_separator(code_at(path, len - 1)) {
        tail.push('\\');
    }
    if !is_absolute && device.is_none() && path.contains(':') {
        // If the original path was not absolute and if we have not been able to
        // resolve it relative to a particular device, we need to ensure that the
        // `tail` has not become something that Windows might interpret as an
        // absolute path. See CVE-2024-36139.
        // `tail.length >= 2` only guarded the two reads, which read none past the end.
        if is_windows_device_root(code_at(&tail, 0)) && code_at(&tail, 1) == Some(b':') {
            return format!(".\\{tail}");
        }
        // Each colon in turn, as `indexOf(':', index + 1)` finds them. `index ===
        // len - 1` asks whether the colon is the last character, which it is by
        // either count: a colon is one byte and one unit.
        for (index, _) in path.match_indices(':') {
            if index == len - 1 || is_path_separator(code_at(path, index + 1)) {
                return format!(".\\{tail}");
            }
        }
    }
    let colon_index = path.find(':');
    if is_windows_reserved_name(path, colon_index) {
        return format!(".\\{}{tail}", device.as_deref().unwrap_or_default());
    }
    match (device, is_absolute) {
        (None, true) => format!("\\{tail}"),
        (None, false) => tail,
        (Some(device), true) => format!("{device}\\{tail}"),
        (Some(device), false) => format!("{device}{tail}"),
    }
}

/// `path.win32.join`.
pub fn join(parts: &[&str]) -> String {
    if parts.is_empty() {
        return ".".to_owned();
    }

    let path: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|part| !part.is_empty())
        .collect();

    let Some(first_part) = path.first() else {
        return ".".to_owned();
    };
    let mut joined = path.join("\\");

    // Make sure that the joined path doesn't start with two slashes, because
    // normalize() will mistake it for a UNC path then.
    //
    // This step is skipped when it is very clear that the user actually
    // intended to point at a UNC path. This is assumed when the first
    // non-empty string arguments starts with exactly two slashes followed by
    // at least one more non-slash character.
    //
    // Note that for normalize() to treat a path as a UNC path it needs to
    // have at least 2 components, so we don't filter for that here.
    // This means that the user can use join to construct UNC paths from
    // a server name and a share name; for example:
    //   path.join('//server', 'share') -> '\\\\server\\share\\')
    let mut needs_replace = true;
    let mut slash_count = 0;
    if is_path_separator(code_at(first_part, 0)) {
        slash_count += 1;
        // `firstLen > 1` and `> 2`: the characters before are separators, one
        // byte and one unit each, so there is another exactly when there is
        // another byte.
        let first_len = first_part.len();
        if first_len > 1 && is_path_separator(code_at(first_part, 1)) {
            slash_count += 1;
            if first_len > 2 {
                if is_path_separator(code_at(first_part, 2)) {
                    slash_count += 1;
                } else {
                    // We matched a UNC path in the first part
                    needs_replace = false;
                }
            }
        }
    }
    if needs_replace {
        // Find any more consecutive slashes we need to replace. `slashCount <
        // joined.length` is an index against the end, and the separators before
        // it are one byte and one unit each.
        while slash_count < joined.len() && is_path_separator(code_at(&joined, slash_count)) {
            slash_count += 1;
        }

        // Replace the slashes if needed
        if slash_count >= 2 {
            joined = format!("\\{}", slice(&joined, slash_count, joined.len()));
        }
    }

    // Skip normalization when reserved device names are present: a part, the
    // text between backslashes, that has a colon after a reserved name.
    let reserved = joined
        .split('\\')
        .filter(|part| !part.is_empty())
        .any(|part| {
            let colon_index = part.find(':');
            colon_index.is_some() && is_windows_reserved_name(part, colon_index)
        });
    if reserved {
        // Replace forward slashes with backslashes
        return joined.replace('/', "\\");
    }

    normalize(&joined)
}
