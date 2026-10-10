//! The files a case sets up before it runs `cf` and looks at afterwards:
//! written, read and copied with the path in whatever goes wrong, and the
//! folders above a file made when it is written.

use std::fs;
use std::path::Path;

use crate::{Error, Result};

/// Makes the folder `path`, and the ones above it that are not there.
pub fn make_dir(path: &Path) -> Result {
    fs::create_dir_all(path).map_err(Error::file("make", path))
}

/// Writes `contents` to the file `path`, in a folder made for it if it is not
/// there, over whatever the file held.
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result {
    if let Some(dir) = path.parent() {
        make_dir(dir)?;
    }
    fs::write(path, contents).map_err(Error::file("write", path))
}

/// Writes `contents` to the file `path` as [`write`] does, and makes it a
/// program a system will start: executable on Unix, where a script is no
/// program without that. Windows starts a file by its name's extension.
pub fn write_executable(path: &Path, contents: impl AsRef<[u8]>) -> Result {
    if let Some(dir) = path.parent() {
        make_dir(dir)?;
    }
    executable(path, contents.as_ref()).map_err(Error::file("write", path))
}

#[cfg(unix)]
fn executable(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o755)
        .open(path)?;
    file.write_all(contents)
}

#[cfg(not(unix))]
fn executable(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    fs::write(path, contents)
}

/// The bytes of the file `path`.
pub fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(Error::file("read", path))
}

/// The text of the file `path`.
pub fn read_string(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(Error::file("read", path))
}

/// Copies the file `from` to `to`, in a folder made for it if it is not there.
pub fn copy(from: &Path, to: &Path) -> Result {
    write(to, read(from)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_written_in_folders_made_for_it_and_over_what_it_held() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("a").join("b").join("c.txt");
        write(&file, "first").unwrap();
        write(&file, b"second").unwrap();
        assert_eq!(read(&file).unwrap(), b"second");
        assert_eq!(read_string(&file).unwrap(), "second");
    }

    #[test]
    fn a_program_is_written_in_folders_made_for_it_and_over_what_it_held() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("bin").join("claude");
        write_executable(&file, "first").unwrap();
        write_executable(&file, "second").unwrap();
        assert_eq!(read_string(&file).unwrap(), "second");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_written_here_is_executable_for_all_and_a_file_is_not() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("program");
        let plain = root.path().join("plain");
        write_executable(&program, "#!/bin/sh\n").unwrap();
        write(&plain, "text").unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o111;
        assert_eq!(mode(&program), 0o111);
        assert_eq!(mode(&plain), 0);
    }

    #[test]
    fn a_file_is_copied_in_a_folder_made_for_it() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from.json");
        write(&from, [0, 159, 146, 150]).unwrap();
        let to = root.path().join("deeper").join("to.json");
        copy(&from, &to).unwrap();
        assert_eq!(read(&to).unwrap(), [0, 159, 146, 150]);
    }

    #[test]
    fn a_folder_is_made_with_the_ones_above_it_and_again_without_complaint() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("x").join("y");
        make_dir(&dir).unwrap();
        make_dir(&dir).unwrap();
        assert!(dir.is_dir());
    }

    #[test]
    fn what_cannot_be_read_is_named_with_its_path() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing.txt");
        for failed in [
            read(&missing).unwrap_err(),
            read_string(&missing).unwrap_err(),
        ] {
            assert!(
                matches!(&failed, Error::File { action: "read", path, .. } if *path == missing),
                "{failed}"
            );
        }
        let not_text = root.path().join("bytes");
        write(&not_text, [255, 254]).unwrap();
        assert!(read_string(&not_text).is_err());
        assert!(copy(&missing, &root.path().join("to")).is_err());
    }
}
