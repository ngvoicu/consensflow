use super::*;
use crate::file::identity;
use std::io;

/// The temporary `write_whole` makes beside `path`.
fn temporary_of(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(format!(".{}.tmp", std::process::id()));
    PathBuf::from(name)
}

/// The names of what `folder` holds, in order.
fn left_in(folder: &Path) -> Vec<String> {
    let mut left: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    left
}

#[test]
fn a_file_written_whole_replaces_the_one_there_and_leaves_nothing_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home").join("agents.json");
    write_whole(&path, b"{}\n").unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        b"{}\n",
        "the folder made, the file written"
    );
    let before = identity(&File::open(&path).unwrap()).unwrap();
    write_whole(&path, b"{\"agents\":[]}\n").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"{\"agents\":[]}\n");
    assert_ne!(
        identity(&File::open(&path).unwrap()).unwrap(),
        before,
        "another file renamed over it, never the old one written in place"
    );
    assert_eq!(left_in(path.parent().unwrap()), ["agents.json"]);
}

#[test]
fn a_folder_that_is_not_there_is_made_with_every_level_above_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a").join("b").join("c").join("agents.json");
    write_whole(&path, b"x").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"x");
    assert_eq!(left_in(path.parent().unwrap()), ["agents.json"]);
}

#[test]
fn a_rename_that_fails_says_the_rename_removes_the_temporary_and_leaves_what_was_there() {
    let dir = tempfile::tempdir().unwrap();
    // A folder with something in it where the file should be.
    let path = dir.path().join("agents.json");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("inside"), "kept").unwrap();
    let failure = write_whole(&path, b"{}\n").unwrap_err();
    let said = failure.to_string();
    assert!(
        said.ends_with(&format!(
            ", rename '{}' -> '{}'",
            temporary_of(&path).display(),
            path.display()
        )),
        "{said}"
    );
    // Windows refuses it as access denied, which libuv calls `EPERM`.
    #[cfg(unix)]
    assert_eq!(
        said,
        format!(
            "EISDIR: illegal operation on a directory, rename '{}' -> '{}'",
            temporary_of(&path).display(),
            path.display()
        )
    );
    assert_eq!(fs::read_to_string(path.join("inside")).unwrap(), "kept");
    assert_eq!(left_in(dir.path()), ["agents.json"], "no temporary left");
}

#[test]
fn a_directory_at_the_temporary_is_said_as_err_fs_eisdir_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agents.json");
    let temporary = temporary_of(&path);
    fs::create_dir(&temporary).unwrap();
    fs::write(temporary.join("inside"), "kept").unwrap();
    let failure = write_whole(&path, b"{}\n").unwrap_err();
    // The open failed first; the removal's failure is the one Node throws.
    assert_eq!(failure.code(), "ERR_FS_EISDIR");
    assert_eq!(
        failure.to_string(),
        format!(
            "Path is a directory: rm returned EISDIR (is a directory) {}",
            temporary.display()
        )
    );
    assert_eq!(
        fs::read_to_string(temporary.join("inside")).unwrap(),
        "kept"
    );
    assert!(!path.exists(), "no file made");
}

#[test]
fn a_file_where_the_folder_should_be_says_the_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("consensflow");
    fs::write(&folder, "a file").unwrap();
    let failure = write_whole(&folder.join("agents.json"), b"{}\n").unwrap_err();
    assert_eq!(
        failure.to_string(),
        format!("EEXIST: file already exists, mkdir '{}'", folder.display())
    );
    assert_eq!(failure.code(), "EEXIST");
    assert_eq!(fs::read_to_string(&folder).unwrap(), "a file");
}

#[test]
fn a_file_in_the_way_of_a_folder_above_says_it_is_not_a_directory_and_the_whole_path() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    fs::write(&file, "a file").unwrap();
    let folder = file.join("a").join("b");
    let failure = write_whole(&folder.join("agents.json"), b"{}\n").unwrap_err();
    assert_eq!(
        failure.to_string(),
        format!("ENOTDIR: not a directory, mkdir '{}'", folder.display())
    );
    assert_eq!(failure.code(), "ENOTDIR");
}

#[test]
fn a_file_in_the_way_exists_for_the_path_itself_and_is_no_directory_for_a_level_above() {
    // Windows says a file exists for each level of the walk; Unix, which
    // answers `ENOTDIR` itself, never gets here with levels below.
    assert_eq!(not_a_folder(Some("EEXIST"), false), "EEXIST");
    assert_eq!(not_a_folder(Some("EEXIST"), true), "ENOTDIR");
    assert_eq!(not_a_folder(Some("ELOOP"), true), "EEXIST");
    assert_eq!(not_a_folder(None, true), "EEXIST");
}

#[test]
fn a_write_that_fails_is_said_with_the_call_and_no_path() {
    struct Full;
    impl io::Write for Full {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            // `ENOSPC` on Unix, `ERROR_DISK_FULL` on Windows.
            Err(io::Error::from_raw_os_error(if cfg!(unix) {
                28
            } else {
                112
            }))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let failure = write_all(&mut Full, b"text").unwrap_err();
    assert_eq!(
        failure.to_string(),
        "ENOSPC: no space left on device, write"
    );
    assert_eq!(failure.code(), "ENOSPC");
}

#[test]
fn what_is_not_there_is_no_failure_to_remove_and_a_file_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    remove(&dir.path().join("none")).unwrap();
    let file = dir.path().join("file");
    fs::write(&file, "x").unwrap();
    remove(&file).unwrap();
    assert!(!file.exists());
    let folder = dir.path().join("folder");
    fs::create_dir(&folder).unwrap();
    assert_eq!(remove(&folder).unwrap_err().code(), "ERR_FS_EISDIR");
    assert!(folder.is_dir());
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A folder whose permissions are `mode` until the guard goes, which
    /// opens it again so that the temporary folder can be removed.
    struct Locked(PathBuf);

    impl Locked {
        fn new(folder: &Path, mode: u32) -> Self {
            fs::set_permissions(folder, fs::Permissions::from_mode(mode)).unwrap();
            Self(folder.to_path_buf())
        }
    }

    impl Drop for Locked {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Whether `folder` keeps this user from making a file in it: root
    /// writes where it likes.
    fn refuses_a_new_file(folder: &Path) -> bool {
        File::create(folder.join("probe")).is_err()
    }

    #[test]
    fn a_folder_that_may_not_be_written_says_the_open_of_the_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("home");
        fs::create_dir(&folder).unwrap();
        let _locked = Locked::new(&folder, 0o555);
        if !refuses_a_new_file(&folder) {
            return;
        }
        let path = folder.join("agents.json");
        let failure = write_whole(&path, b"{}\n").unwrap_err();
        assert_eq!(
            failure.to_string(),
            format!(
                "EACCES: permission denied, open '{}'",
                temporary_of(&path).display()
            )
        );
        assert_eq!(failure.code(), "EACCES");
        assert_eq!(left_in(&folder), Vec::<String>::new());
    }

    #[test]
    fn a_folder_that_may_not_be_made_in_says_the_mkdir_of_the_whole_path() {
        let dir = tempfile::tempdir().unwrap();
        let above = dir.path().join("ro");
        fs::create_dir(&above).unwrap();
        let _locked = Locked::new(&above, 0o555);
        if !refuses_a_new_file(&above) {
            return;
        }
        let folder = above.join("x").join("y");
        let failure = write_whole(&folder.join("agents.json"), b"{}\n").unwrap_err();
        // Not the level that failed, `ro/x`: the whole path, as Node says it.
        assert_eq!(
            failure.to_string(),
            format!("EACCES: permission denied, mkdir '{}'", folder.display())
        );
    }

    #[test]
    fn a_folder_that_may_not_be_entered_says_the_failed_look_in_place_of_the_failed_open() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("locked");
        fs::create_dir(&folder).unwrap();
        let _locked = Locked::new(&folder, 0o000);
        if fs::read_dir(&folder).is_ok() {
            return;
        }
        let path = folder.join("agents.json");
        let failure = write_whole(&path, b"{}\n").unwrap_err();
        // The open says EACCES first; looking at the temporary to remove it
        // fails too, and that is what Node throws: `lstat`, not `open`.
        assert_eq!(
            failure.to_string(),
            format!(
                "EACCES: permission denied, lstat '{}'",
                temporary_of(&path).display()
            )
        );
    }

    #[test]
    fn a_stale_temporary_in_a_read_only_folder_is_a_removal_refused_and_said_instead_of_the_rename()
    {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("home");
        fs::create_dir(&folder).unwrap();
        let path = folder.join("agents.json");
        let temporary = temporary_of(&path);
        // The file is there to be written over, which a read-only folder
        // allows; making and removing entries it does not.
        fs::write(&temporary, "stale").unwrap();
        let _locked = Locked::new(&folder, 0o555);
        if !refuses_a_new_file(&folder) {
            return;
        }
        let failure = write_whole(&path, b"{}\n").unwrap_err();
        assert_eq!(
            failure.to_string(),
            format!("EACCES, Permission denied: {0} '{0}'", temporary.display())
        );
        assert_eq!(failure.code(), "EACCES");
        assert_eq!(
            left_in(&folder),
            [temporary.file_name().unwrap().to_str().unwrap()]
        );
        assert_eq!(fs::read_to_string(&temporary).unwrap(), "{}\n");
    }
}

#[cfg(windows)]
#[test]
fn a_folder_name_windows_refuses_ends_the_walk_where_node_ends_it() {
    // libuv's mkdir says EINVAL for it, and the walk asks what is there: one
    // that climbed above it would make the folder above, and climb again.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("bad<name");
    let path = folder.join("agents.json");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(write_whole(&path, b"{}\n").map_err(|error| error.to_string()));
    });
    let said = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the walk ends");
    assert_eq!(
        said.unwrap_err(),
        format!(
            "ENOENT: no such file or directory, mkdir '{}'",
            folder.display()
        )
    );
}
