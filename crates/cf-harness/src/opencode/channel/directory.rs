//! A folder as the system names it (`realpath` of `fs/promises`): the folder
//! a window works in, which OpenCode's server names its sessions' folders by
//! too (the one a request says and the one a session comes back with must
//! compare equal, whether the folder is reached through a link or not).

use std::path::Path;

use cf_base::file::FileError;

/// `path` with its links followed, as the system writes it, or why it could
/// not be made, in Node's words (`ENOENT: no such file or directory,
/// realpath '…'`).
pub(super) fn real_path(path: &str) -> Result<String, String> {
    let real = std::fs::canonicalize(path)
        .map_err(|failed| FileError::call(failed, "realpath", Some(Path::new(path))).to_string())?;
    let named = real.to_string_lossy();
    Ok(if cfg!(windows) {
        plain(&named)
    } else {
        named.into_owned()
    })
}

/// A Windows name as libuv writes it: the `\\?\` of a drive's path taken off
/// (`\\?\C:\x` is `C:\x`), and `\\?\UNC\server\share` as `\\server\share`.
fn plain(name: &str) -> String {
    if let Some(rest) = name.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match name.strip_prefix(r"\\?\") {
        Some(rest) if is_drive(rest) => rest.to_owned(),
        _ => name.to_owned(),
    }
}

/// Whether `text` begins with a drive's letter and its colon.
fn is_drive(text: &str) -> bool {
    let mut characters = text.chars();
    matches!(
        (characters.next(), characters.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_that_is_there_is_its_real_name_and_one_that_is_not_says_so_in_node_s_words() {
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        let named = real_path(&dir.path().to_string_lossy()).unwrap();
        assert_eq!(named, plain_on_windows(&real.to_string_lossy()));
        let missing = dir.path().join("not-there").to_string_lossy().into_owned();
        let failed = real_path(&missing).unwrap_err();
        assert_eq!(
            failed,
            format!("ENOENT: no such file or directory, realpath '{missing}'")
        );
    }

    /// What a real path is written as here.
    fn plain_on_windows(name: &str) -> String {
        if cfg!(windows) {
            plain(name)
        } else {
            name.to_owned()
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_followed_to_the_folder_it_leads_to() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let expected = std::fs::canonicalize(&target).unwrap();
        assert_eq!(
            real_path(&link.to_string_lossy()).unwrap(),
            expected.to_string_lossy()
        );
    }

    #[test]
    fn a_verbatim_name_loses_its_prefix_as_libuv_takes_it_off() {
        assert_eq!(plain(r"\\?\C:\Users\me"), r"C:\Users\me");
        assert_eq!(plain(r"\\?\UNC\server\share\x"), r"\\server\share\x");
        // Not a drive, and not a share: left as it is.
        assert_eq!(plain(r"\\?\Volume{1}\x"), r"\\?\Volume{1}\x");
        assert_eq!(plain(r"C:\Users\me"), r"C:\Users\me");
        assert_eq!(plain(r"\\server\share"), r"\\server\share");
    }
}
