//! The files a case sets up before it runs `cf` and looks at afterwards:
//! written, read and copied with the path in whatever goes wrong, and the
//! folders above a file made when it is written.

use std::path::{Path, PathBuf};
use std::{fs, io};

use crate::{Error, Result};

/// Makes the folder `path`, and the ones above it that are not there: the door's own
/// ([`crate::door::make_dir`]), below a folder this run made.
pub fn make_dir(path: &Path) -> Result {
    crate::door::make_dir(path)
}

/// Writes `contents` to the file `path`, in a folder made for it if it is not there, over
/// whatever the file held: the door's own ([`crate::door::write`]), below a folder this run made.
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result {
    crate::door::write(path, contents)
}

/// Writes `text` to the file `file` whole or not at all ([`crate::door::write_whole`]).
pub fn write_whole(file: &Path, text: &str) -> Result {
    crate::door::write_whole(file, text)
}

/// Writes the process id of this program to `<file>.pid`, whole ([`crate::door::say_its_process`]).
pub fn say_its_process(file: &Path) -> Result {
    crate::door::say_its_process(file)
}

/// Writes `contents` to the file `path` as [`write`] does, and makes it a program a system will
/// start ([`crate::door::write_executable`]).
pub fn write_executable(path: &Path, contents: impl AsRef<[u8]>) -> Result {
    crate::door::write_executable(path, contents)
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

/// Copies the file `from` to `to`, in a folder made for it if it is not there
/// ([`crate::door::copy`]).
pub fn copy(from: &Path, to: &Path) -> Result {
    crate::door::copy(from, to)
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

/// `path` by its real one, its links resolved: Muse takes a data folder reached through a
/// link for unsafe (`/tmp` on a Mac, where a plant run makes its rigs, T-829). As given on
/// Windows, where the real path is a verbatim one (`\\?\`) that programs do not take
/// (T-931), and where it cannot be read. The product's rule is `cf_base::file::real_path`.
pub fn real_path(path: &Path) -> PathBuf {
    real_path_on(path, cfg!(windows))
}

/// [`real_path`], on Windows where `windows` says so.
fn real_path_on(path: &Path, windows: bool) -> PathBuf {
    if windows {
        return path.to_path_buf();
    }
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A real name of a folder (`canonical`, what `fs::canonicalize` says of it) as the product
/// writes it in a note, which is the name Node's `realpath` gives (`cf_harness`'s
/// `real_path`): on Windows the `\\?\` of a drive's path taken off (`\\?\C:\x` is `C:\x`) and
/// `\\?\UNC\server\share` as `\\server\share`. What a note names is found by this name, and
/// not by the canonical one, which no note holds there (T-1079: three trust notes that were
/// there were not found).
pub fn plain_name(canonical: &str) -> String {
    if let Some(share) = canonical.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}");
    }
    match canonical.strip_prefix(r"\\?\") {
        Some(rest) if has_drive(rest) => rest.to_owned(),
        _ => canonical.to_owned(),
    }
}

/// Whether `text` begins with a drive's letter and its colon.
fn has_drive(text: &str) -> bool {
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
    fn a_real_path_is_as_given_on_windows_whose_real_paths_are_verbatim_ones() {
        let folder = crate::door::made("t-").unwrap();
        assert_eq!(real_path_on(folder.path(), true), folder.path());
        assert_eq!(
            real_path_on(folder.path(), false),
            fs::canonicalize(folder.path()).unwrap()
        );
    }

    #[test]
    fn a_folder_is_named_as_the_product_writes_it_with_no_verbatim_prefix_of_a_drive_or_a_share() {
        // What Windows' `canonicalize` says of a folder, and the name Node's `realpath` gives it.
        assert_eq!(plain_name(r"\\?\C:\Users\me\live\x"), r"C:\Users\me\live\x");
        assert_eq!(
            plain_name(r"\\?\UNC\server\share\live"),
            r"\\server\share\live"
        );
        // A name that is none of those two is as given, whatever the system.
        assert_eq!(plain_name(r"\\?\Volume{1b2}\x"), r"\\?\Volume{1b2}\x");
        assert_eq!(plain_name(r"\\?\1:\x"), r"\\?\1:\x");
        assert_eq!(plain_name("/private/tmp/x"), "/private/tmp/x");
        assert_eq!(plain_name(r"C:\Users\me"), r"C:\Users\me");
    }

    #[test]
    fn a_file_is_written_in_folders_made_for_it_and_over_what_it_held() {
        let root = crate::door::made("t-").unwrap();
        let file = root.path().join("a").join("b").join("c.txt");
        write(&file, "first").unwrap();
        write(&file, b"second").unwrap();
        assert_eq!(read(&file).unwrap(), b"second");
        assert_eq!(read_string(&file).unwrap(), "second");
    }

    #[test]
    fn a_program_is_written_in_folders_made_for_it_and_over_what_it_held() {
        let root = crate::door::made("t-").unwrap();
        let file = root.path().join("bin").join("claude");
        write_executable(&file, "first").unwrap();
        write_executable(&file, "second").unwrap();
        assert_eq!(read_string(&file).unwrap(), "second");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_written_here_is_executable_for_all_and_a_file_is_not() {
        use std::os::unix::fs::PermissionsExt;

        let root = crate::door::made("t-").unwrap();
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
        let root = crate::door::made("t-").unwrap();
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
        let root = crate::door::made("t-").unwrap();
        let from = root.path().join("from.json");
        write(&from, [0, 159, 146, 150]).unwrap();
        let to = root.path().join("deeper").join("to.json");
        copy(&from, &to).unwrap();
        assert_eq!(read(&to).unwrap(), [0, 159, 146, 150]);
    }

    #[test]
    fn a_file_is_found_by_its_name_in_the_folders_below_a_root_and_none_where_there_is_none() {
        let root = crate::door::made("t-").unwrap();
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
        let root = crate::door::made("t-").unwrap();
        let dir = root.path().join("x").join("y");
        make_dir(&dir).unwrap();
        make_dir(&dir).unwrap();
        assert!(dir.is_dir());
    }

    #[test]
    fn what_cannot_be_read_is_named_with_its_path() {
        let root = crate::door::made("t-").unwrap();
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
        let root = crate::door::made("t-").unwrap();
        let file = root.path().join("said");
        let mut writing = file.as_os_str().to_owned();
        writing.push(".writing");
        let made = crate::process::Run::new("mkfifo")
            .arg(&writing)
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
        let root = crate::door::made("t-").unwrap();
        let named = root.path().join("job-1-1");
        say_its_process(&named).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("job-1-1.pid")).unwrap(),
            std::process::id().to_string()
        );
        assert!(!root.path().join("job-1-1.pid.writing").exists());
    }
}
