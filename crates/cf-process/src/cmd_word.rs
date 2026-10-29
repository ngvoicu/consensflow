//! A path as one word of a cmd.exe line with no quotes in it: what a program that hands
//! cmd.exe its line through Rust's own quoting (Muse's hooks on Windows, T-792) can be
//! given, for there every `"` of the line reaches cmd.exe as `\"`, and no quoted word
//! survives.

use std::path::Path;

/// `path` as one word cmd.exe reads as it is: itself where it holds no blank and nothing
/// cmd.exe reads as its own (`" & | < > ^ % ! ( ) , ; =`), else on Windows its short
/// form (`C:\PROGRA~1\…`), the folders that are there named so and the rest as given.
/// None where neither is such a word: a short form Windows does not keep (8.3 names
/// turned off), or one that still holds such a character.
pub fn cmd_word(path: &Path) -> Option<String> {
    let given = path.to_string_lossy().into_owned();
    if plain(&given) {
        return Some(given);
    }
    let short = short(path)?;
    plain(&short).then_some(short)
}

/// Whether `word` holds no blank and none of the characters cmd.exe reads as its own.
fn plain(word: &str) -> bool {
    !word.is_empty()
        && !word
            .chars()
            .any(|c| c.is_whitespace() || "\"&|<>^%!(),;=".contains(c))
}

/// The short form of `path`: the nearest folder of it that is there in its short form,
/// and the rest of it as given.
#[cfg(windows)]
fn short(path: &Path) -> Option<String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let mut rest = Vec::new();
    for ancestor in path.ancestors() {
        let wide: Vec<u16> = ancestor.as_os_str().encode_wide().chain([0]).collect();
        let mut buffer = vec![0u16; 32_768];
        let size = u32::try_from(buffer.len()).ok()?;
        #[allow(unsafe_code)]
        // SAFETY: `wide` is the path ended by its 0, `buffer` ours, of the size passed;
        // the call writes at most that many units into it and no other memory of ours.
        let written = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), size) };
        let written = usize::try_from(written).ok()?;
        if written > 0 && written < buffer.len() {
            let mut short =
                std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..written]));
            for part in rest.iter().rev() {
                short.push(part);
            }
            return Some(short.to_string_lossy().into_owned());
        }
        rest.push(ancestor.file_name()?.to_owned());
    }
    None
}

#[cfg(not(windows))]
fn short(_path: &Path) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::cmd_word;

    #[test]
    fn a_plain_path_is_its_own_word_and_one_cmd_would_split_is_none_where_no_short_form_is() {
        assert_eq!(
            cmd_word(Path::new(r"C:\Users\Zoë\l\sessions.jsonl")).as_deref(),
            Some(r"C:\Users\Zoë\l\sessions.jsonl")
        );
        for split in [
            r"Z:\no such\cf.exe",
            r"Z:\a&b\cf.exe",
            r"Z:\100%\cf.exe",
            "",
        ] {
            assert_eq!(cmd_word(Path::new(split)), None, "{split:?}");
        }
    }

    /// On Windows a folder with a blank in its name is named by its short form, the part
    /// of the path not there yet as given.
    #[cfg(windows)]
    #[test]
    fn on_windows_a_path_with_a_blank_is_named_by_its_short_form() {
        let folder = tempfile::tempdir().unwrap();
        let spaced = folder.path().join("Consens Flow");
        std::fs::create_dir(&spaced).unwrap();
        let file = spaced.join("sessions.jsonl");
        let word = cmd_word(&file).expect("a short form (8.3 names are on for the test's drive)");
        assert!(
            !word.contains(' ') && word.ends_with(r"\sessions.jsonl"),
            "{word}"
        );
        std::fs::write(&file, "x").unwrap();
        assert_eq!(std::fs::read_to_string(&word).unwrap(), "x");
    }
}
