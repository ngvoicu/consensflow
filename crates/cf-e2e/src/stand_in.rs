//! A harness's command line that does nothing, for a PATH to find. `cf setup`
//! and `cf doctor` look for the harnesses they know on the PATH, and a case that
//! needs one found has no use for it to run: it exits 0 and says nothing. A
//! shell script on POSIX, a `.cmd` on Windows, which has no shebang.

use std::path::{Path, PathBuf};

use crate::{files, Result};

/// What the script says on POSIX.
#[cfg(unix)]
const SCRIPT: &[u8] = b"#!/bin/sh\nexit 0\n";

/// What the `.cmd` says on Windows.
#[cfg(windows)]
const SCRIPT: &[u8] = b"@echo off\r\nexit /b 0\r\n";

/// Puts a stand-in for the command `name` in `dir` (made if it is not there):
/// the path it is found at, `name` itself on POSIX and `name.cmd` on Windows.
pub fn install(dir: &Path, name: &str) -> Result<PathBuf> {
    let path = dir.join(program_name(name));
    files::write_executable(&path, SCRIPT)?;
    Ok(path)
}

/// The file a command `name` is found at: `name` itself on POSIX, `name.cmd`
/// on Windows, which has no shebang and starts a script by its extension.
pub fn program_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::Error;

    #[test]
    fn a_stand_in_is_made_in_a_folder_that_is_made_for_it_under_the_name_a_system_finds_it_by() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("not").join("yet");
        let path = install(&dir, "claude").unwrap();
        let name = if cfg!(windows) {
            "claude.cmd"
        } else {
            "claude"
        };
        assert_eq!(path, dir.join(name));
        assert_eq!(program_name("claude"), name);
        assert_eq!(fs::read(&path).unwrap(), SCRIPT);
    }

    /// Run by the shell and not started as a program: a script that has just
    /// been written, started by a test while another thread starts a program
    /// of its own, may be refused as a file still open for writing.
    #[cfg(unix)]
    #[test]
    fn a_stand_in_is_an_executable_script_that_ends_at_once_with_0_and_says_nothing() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let path = install(root.path(), "claude").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o111,
            0o111
        );
        let ran = crate::process::Run::new("/bin/sh")
            .arg(&path)
            .run()
            .unwrap();
        assert_eq!(
            (ran.code, ran.stdout.as_str(), ran.stderr.as_str()),
            (Some(0), "", "")
        );
    }

    #[test]
    fn a_stand_in_cannot_be_put_where_a_file_is_in_the_way() {
        let root = tempfile::tempdir().unwrap();
        let in_the_way = root.path().join("bin");
        fs::write(&in_the_way, "a file").unwrap();
        let failed = install(&in_the_way, "claude").unwrap_err();
        assert!(
            matches!(failed, Error::File { action: "make", .. }),
            "{failed}"
        );
    }
}
