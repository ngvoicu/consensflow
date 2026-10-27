//! The files a case sets up before it runs `cf` and looks at afterwards:
//! written, read and copied with the path in whatever goes wrong, and the
//! folders above a file made when it is written.

use std::path::{Path, PathBuf};
use std::{fs, io};

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

/// Writes `text` to the file `file` whole or not at all, for a program to say something to
/// a case that waits for the file and reads it at once: under another name, then renamed
/// into place. `fs::write` makes the file before it writes into it, and what a case reads
/// in that moment is nothing, which is no pid.
pub fn write_whole(file: &Path, text: &str) -> io::Result<()> {
    let mut writing = file.as_os_str().to_owned();
    writing.push(".writing");
    fs::write(&writing, text)?;
    fs::rename(writing, file)
}

/// Writes the process id of this program to `<file>.pid`, whole ([`write_whole`]): the
/// file in which a stand-in tells a case which process it is.
pub fn say_its_process(file: &Path) -> io::Result<()> {
    let mut named = file.as_os_str().to_owned();
    named.push(".pid");
    write_whole(Path::new(&named), &std::process::id().to_string())
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

/// The text of the file `path` up to its last line break: the lines of a file
/// another program is still appending to that it has finished. Read while it
/// writes, the file may end in the start of a line (a JSON record cut inside a
/// string, or inside a letter's bytes); that tail is left out, and the next read
/// has the line whole.
pub fn read_finished_lines(path: &Path) -> Result<String> {
    let mut bytes = read(path)?;
    bytes.truncate(
        bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |at| at + 1),
    );
    String::from_utf8(bytes).map_err(|invalid| {
        Error::file("read", path)(io::Error::new(io::ErrorKind::InvalidData, invalid))
    })
}

/// Copies the file `from` to `to`, in a folder made for it if it is not there.
pub fn copy(from: &Path, to: &Path) -> Result {
    write(to, read(from)?)
}

/// The first file under the folder `root`, or in the folders under it, whose name
/// `wanted` takes: none where none is, or `root` is not there. What finds a
/// record that a program keeps in the folder of the day it was begun, by a name
/// that holds something of its own (a Codex's rollout holds its thread), where a
/// case does not know the day.
pub fn find(root: &Path, wanted: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find(&path, wanted) {
                return Some(found);
            }
        } else if path
            .file_name()
            .is_some_and(|name| wanted(&name.to_string_lossy()))
        {
            return Some(path);
        }
    }
    None
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
    fn only_the_lines_a_writer_has_finished_are_read_and_a_torn_end_waits_for_the_next_read() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("record.jsonl");
        let finished = "{\"a\":1}\n{\"b\":\"é\"}\n";
        // A line cut inside a string, as a reader that came between two of its
        // writes meets it: the lines before it are read, it is not.
        write(&file, format!("{finished}{{\"c\":\"tor")).unwrap();
        assert_eq!(read_finished_lines(&file).unwrap(), finished);
        // Cut inside the bytes of a letter: no text at all, but the lines before it.
        let mut cut = format!("{finished}{{\"c\":\"").into_bytes();
        cut.push(0xC3);
        write(&file, cut).unwrap();
        assert_eq!(read_finished_lines(&file).unwrap(), finished);
        // The writer ends the line: the next read has it whole.
        let whole = format!("{finished}{{\"c\":\"torn\"}}\n");
        write(&file, &whole).unwrap();
        assert_eq!(read_finished_lines(&file).unwrap(), whole);
        // No line is finished yet, or none is written.
        write(&file, "{\"a\":").unwrap();
        assert_eq!(read_finished_lines(&file).unwrap(), "");
        write(&file, "").unwrap();
        assert_eq!(read_finished_lines(&file).unwrap(), "");
        // A file that is not there is named, as any read names it.
        let missing = root.path().join("missing.jsonl");
        assert!(matches!(
            read_finished_lines(&missing),
            Err(Error::File { action: "read", .. })
        ));
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
    fn a_file_is_found_by_its_name_in_the_folders_below_a_root_and_none_where_there_is_none() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let day = sessions.join("2026").join("10").join("13");
        write(&day.join("rollout-at-0001.jsonl"), "one").unwrap();
        write(&sessions.join("2026").join("note.txt"), "no rollout").unwrap();
        let ends = |end: &'static str| move |name: &str| name.ends_with(end);
        assert_eq!(
            find(&sessions, &ends("-0001.jsonl")),
            Some(day.join("rollout-at-0001.jsonl"))
        );
        assert_eq!(find(&sessions, &ends("-0002.jsonl")), None);
        // A folder is not a file by its name, and a root that is not there holds none.
        assert_eq!(find(&sessions, &ends("13")), None);
        assert_eq!(find(&root.path().join("nowhere"), &ends("")), None);
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

    /// A file written whole is not there while it is being written: the write is held half
    /// way (its first name is a pipe nobody reads yet), and the file is not there; it is there
    /// once the write is done. `fs::write` to the file itself makes it at once, and a case
    /// that looks for the file finds it before it holds what was meant.
    #[cfg(unix)]
    #[test]
    fn a_file_written_whole_is_not_there_until_it_is_written() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("said");
        let mut writing = file.as_os_str().to_owned();
        writing.push(".writing");
        let made = crate::process::Run::new("mkfifo")
            .arg(&writing)
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(made.code, Some(0), "{made}");
        let (starting, started) = std::sync::mpsc::channel();
        let writer = {
            let file = file.clone();
            std::thread::spawn(move || {
                starting.send(()).unwrap();
                write_whole(&file, "written")
            })
        };
        started.recv().unwrap();
        // Long enough for a write that was not held to be done: this is the check that
        // goes red when the file is written in place.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!file.exists(), "the file was there before it was written");
        // The reader of the pipe lets the write through, and the file is there.
        assert_eq!(fs::read_to_string(Path::new(&writing)).unwrap(), "written");
        writer.join().unwrap().unwrap();
        assert!(file.exists());
    }

    #[test]
    fn a_program_says_its_process_in_the_file_beside_its_name() {
        let root = tempfile::tempdir().unwrap();
        let named = root.path().join("job-1-1");
        say_its_process(&named).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("job-1-1.pid")).unwrap(),
            std::process::id().to_string()
        );
        assert!(!root.path().join("job-1-1.pid.writing").exists());
    }
}
