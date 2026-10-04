use super::*;

/// What the system gives for each failure these tests need said, and the
/// name libuv has for it.
#[cfg(unix)]
mod system {
    /// `EACCES`.
    pub const DENIED: i32 = libc::EACCES;
    /// `EISDIR`.
    pub const IS_A_DIRECTORY: i32 = libc::EISDIR;
    /// `ENOSPC`.
    pub const FULL: i32 = libc::ENOSPC;
    /// `EEXIST`.
    pub const EXISTS: i32 = libc::EEXIST;
    /// An errno libuv has no name for.
    pub const UNNAMED: i32 = libc::ESTALE;
}

#[cfg(windows)]
mod system {
    /// `ERROR_CANT_ACCESS_FILE`: `EACCES`.
    pub const DENIED: i32 = 1920;
    /// `ERROR_INVALID_FUNCTION`: `EISDIR`.
    pub const IS_A_DIRECTORY: i32 = 1;
    /// `ERROR_DISK_FULL`: `ENOSPC`.
    pub const FULL: i32 = 112;
    /// `ERROR_ALREADY_EXISTS`: `EEXIST`.
    pub const EXISTS: i32 = 183;
    /// `ERROR_IO_PENDING`, which libuv does not translate.
    pub const UNNAMED: i32 = 997;
}

fn failure(code: i32) -> io::Error {
    io::Error::from_raw_os_error(code)
}

#[test]
fn a_call_with_a_path_is_said_with_libuvs_name_and_words_the_call_and_the_path() {
    let said = FileError::call(
        failure(system::DENIED),
        "open",
        Some(Path::new("/home/me/.consensflow/agents.json.4242.tmp")),
    );
    assert_eq!(
        said.to_string(),
        "EACCES: permission denied, open '/home/me/.consensflow/agents.json.4242.tmp'"
    );
    assert_eq!(said.code(), "EACCES");
}

#[test]
fn a_call_with_a_destination_says_it_after_the_path() {
    let said = FileError::call_to(
        failure(system::IS_A_DIRECTORY),
        "rename",
        Path::new("/home/me/agents.json.4242.tmp"),
        Path::new("/home/me/agents.json"),
    );
    assert_eq!(
        said.to_string(),
        "EISDIR: illegal operation on a directory, rename '/home/me/agents.json.4242.tmp' -> '/home/me/agents.json'"
    );
    assert_eq!(said.code(), "EISDIR");
}

#[test]
fn a_call_with_no_path_says_the_call_alone() {
    let said = FileError::call(failure(system::FULL), "write", None);
    assert_eq!(said.to_string(), "ENOSPC: no space left on device, write");
    assert_eq!(said.code(), "ENOSPC");
}

#[test]
fn a_failure_libuv_has_no_name_for_is_an_unknown_error() {
    // Node's `uvUnmappedError`, which no golden reaches.
    let said = FileError::call(failure(system::UNNAMED), "open", Some(Path::new("/a/b")));
    assert_eq!(said.to_string(), "UNKNOWN: unknown error, open '/a/b'");
    assert_eq!(said.code(), "UNKNOWN");
    let no_system_code = FileError::call(io::Error::other("no code"), "write", None);
    assert_eq!(no_system_code.to_string(), "UNKNOWN: unknown error, write");
    assert_eq!(no_system_code.code(), "UNKNOWN");
}

#[test]
fn a_failure_said_by_a_name_of_its_own_keeps_the_one_the_system_gave() {
    let said = FileError::named(
        "ENOTDIR",
        failure(system::EXISTS),
        "mkdir",
        Some(Path::new("/a/file/b")),
        None,
    );
    assert_eq!(
        said.to_string(),
        "ENOTDIR: not a directory, mkdir '/a/file/b'"
    );
    assert_eq!(said.code(), "ENOTDIR");
    let source = said
        .source()
        .and_then(|source| source.downcast_ref::<io::Error>());
    assert_eq!(
        source.and_then(io::Error::raw_os_error),
        Some(system::EXISTS)
    );
}

#[test]
fn a_directory_where_rm_was_to_remove_a_file_is_node_s_err_fs_eisdir() {
    let said = FileError::directory(Path::new("/home/me/agents.json.4242.tmp"));
    assert_eq!(
        said.to_string(),
        "Path is a directory: rm returned EISDIR (is a directory) /home/me/agents.json.4242.tmp"
    );
    assert_eq!(said.code(), "ERR_FS_EISDIR");
    assert_eq!(
        said.source()
            .and_then(|source| source.downcast_ref::<io::Error>())
            .map(io::Error::kind),
        Some(io::ErrorKind::IsADirectory)
    );
}

#[cfg(unix)]
#[test]
fn a_removal_the_system_refuses_is_said_as_rm_syncs_cpp_says_it() {
    // Node 26.7.0: a sentence for each of four refusals, else `Unknown
    // error`, the system's words and no code.
    let path = Path::new("/a/t");
    for (errno, text, code) in [
        (
            libc::EACCES,
            "EACCES, Permission denied: /a/t '/a/t'",
            "EACCES",
        ),
        (
            libc::EPERM,
            "EPERM, Operation not permitted: /a/t '/a/t'",
            "EPERM",
        ),
        (
            libc::ENOTEMPTY,
            "ENOTEMPTY, Directory not empty: /a/t '/a/t'",
            "ENOTEMPTY",
        ),
        (
            libc::ENOTDIR,
            "ENOTDIR, Not a directory: /a/t '/a/t'",
            "ENOTDIR",
        ),
        (
            libc::EROFS,
            ", Unknown error: Read-only file system '/a/t'",
            "",
        ),
    ] {
        let said = FileError::removal(io::Error::from_raw_os_error(errno), path);
        assert_eq!((said.to_string().as_str(), said.code()), (text, code));
    }
}

#[cfg(windows)]
#[test]
fn a_removal_windows_refuses_is_said_as_rm_syncs_cpp_says_it_there() {
    let path = Path::new(r"C:\a\t");
    let said = |code: i32| FileError::removal(io::Error::from_raw_os_error(code), path);
    // Access denied and a sharing violation are `permission_denied`: EPERM there.
    assert_eq!(
        said(5).to_string(),
        r"EPERM, Permission denied: \\?\C:\a\t '\\?\C:\a\t'"
    );
    assert_eq!(said(32).code(), "EPERM");
    assert_eq!(
        said(145).to_string(),
        r"ENOTEMPTY, Directory not empty: \\?\C:\a\t '\\?\C:\a\t'"
    );
    // A lock violation is none of the four.
    let unknown = said(33).to_string();
    assert!(unknown.starts_with(", Unknown error: "), "{unknown}");
    assert!(
        unknown.ends_with(r" '\\?\C:\a\t'") && !unknown.contains("os error"),
        "{unknown}"
    );
}

#[test]
fn the_failure_the_system_gave_is_the_source() {
    let said = FileError::call(failure(system::FULL), "write", None);
    let source = said
        .source()
        .and_then(|source| source.downcast_ref::<io::Error>());
    assert_eq!(source.and_then(io::Error::raw_os_error), Some(system::FULL));
}

#[test]
fn the_system_s_words_are_rust_s_text_without_the_code_it_adds() {
    #[cfg(unix)]
    assert_eq!(system_words(&failure(system::DENIED)), "Permission denied");
    assert_eq!(system_words(&io::Error::other("a text")), "a text");
}
