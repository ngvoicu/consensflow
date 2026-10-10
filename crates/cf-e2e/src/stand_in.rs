//! A harness's command line that does nothing, for a PATH to find. `cf setup`
//! and `cf doctor` look for the harnesses they know on the PATH, and a case that
//! needs one found has no use for it to run: it exits 0 and says nothing. A
//! shell script on POSIX, a `.cmd` on Windows, which has no shebang.
//!
//! A command the daemon opens a window on ([`install_for_window`]) is another
//! matter on Windows: it opens only on the shape of an npm shim
//! ([`window_shim`]), and refuses any other `.cmd`, this one included.

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

/// Puts a stand-in for the harness command `name` in `dir` (made if it is not
/// there) that the daemon can open a window on, where a case answers for the
/// pane host and so nothing runs it: the path it is found at. On POSIX any
/// program opens as it is, so it is [`install`]'s script; on Windows it is
/// [`window_shim`], naming `agent`.
pub fn install_for_window(dir: &Path, name: &str, agent: &Path) -> Result<PathBuf> {
    if !cfg!(windows) {
        return install(dir, name);
    }
    let path = dir.join(program_name(name));
    files::write_executable(&path, window_shim(agent))?;
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

/// What the `.cmd` of a command a window opens on says on Windows. The pane
/// host starts a file with each argument quoted the way programs read them,
/// which cmd.exe does not, so the daemon opens a window on a `.cmd` only as
/// the shim npm writes, `"<program>" "<script>" %*`, which it reads for the
/// program and the script and starts those itself; it refuses every other
/// `.cmd`, since only cmd.exe could run it, and then asks the host for no
/// window. This one names `agent`, a file that is there, as both: an agent
/// that is run through it is given its own file first, and knows it.
pub fn window_shim(agent: &Path) -> String {
    let named = agent.display();
    format!("@echo off\r\n\"{named}\" \"{named}\" %*\r\n")
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

    #[test]
    fn a_stand_in_for_a_window_is_the_script_on_posix_and_the_shim_of_the_agent_on_windows() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("not").join("yet");
        let agent = Path::new("/bin/fake-agent");
        let path = install_for_window(&dir, "claude", agent).unwrap();
        assert_eq!(path, dir.join(program_name("claude")));
        let wanted = if cfg!(windows) {
            window_shim(agent).into_bytes()
        } else {
            SCRIPT.to_vec()
        };
        assert_eq!(fs::read(&path).unwrap(), wanted);
    }

    #[test]
    fn a_window_opens_on_a_shim_that_names_the_agent_as_its_program_and_its_script() {
        // The last line that passes the arguments on, in the shape npm writes it, with
        // the batch file's own line ends.
        assert_eq!(
            window_shim(Path::new("/bin/fake-agent")),
            "@echo off\r\n\"/bin/fake-agent\" \"/bin/fake-agent\" %*\r\n"
        );
        // A path with spaces is quoted whole.
        assert_eq!(
            window_shim(Path::new(r"C:\Program Files\cf\fake-agent.exe")),
            "@echo off\r\n\"C:\\Program Files\\cf\\fake-agent.exe\" \
             \"C:\\Program Files\\cf\\fake-agent.exe\" %*\r\n"
        );
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
