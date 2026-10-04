use super::*;
use windows_sys::Win32::Foundation::ERROR_IO_PENDING;

fn named(code: u32) -> Option<&'static str> {
    name(i32::try_from(code).unwrap())
}

#[test]
fn a_win32_code_is_the_name_libuv_translates_it_to() {
    for (code, libuv) in [
        (ERROR_FILE_NOT_FOUND, "ENOENT"),
        (ERROR_PATH_NOT_FOUND, "ENOENT"),
        (ERROR_INVALID_NAME, "ENOENT"),
        // Access denied is a permission nobody has: `EPERM`, not `EACCES`.
        (ERROR_ACCESS_DENIED, "EPERM"),
        (ERROR_CANT_ACCESS_FILE, "EACCES"),
        (ERROR_ALREADY_EXISTS, "EEXIST"),
        (ERROR_FILE_EXISTS, "EEXIST"),
        (ERROR_DIR_NOT_EMPTY, "ENOTEMPTY"),
        (ERROR_SHARING_VIOLATION, "EBUSY"),
        (ERROR_LOCK_VIOLATION, "EBUSY"),
        (ERROR_DISK_FULL, "ENOSPC"),
        (ERROR_HANDLE_DISK_FULL, "ENOSPC"),
        (ERROR_IO_DEVICE, "EIO"),
        (ERROR_CRC, "EIO"),
        (ERROR_WRITE_PROTECT, "EROFS"),
        (ERROR_NOT_SAME_DEVICE, "EXDEV"),
        (ERROR_INVALID_FUNCTION, "EISDIR"),
        (ERROR_FILENAME_EXCED_RANGE, "ENAMETOOLONG"),
        (ERROR_CANT_RESOLVE_FILENAME, "ELOOP"),
        (ERROR_TOO_MANY_OPEN_FILES, "EMFILE"),
        (ERROR_NOT_ENOUGH_MEMORY, "ENOMEM"),
        (ERROR_BROKEN_PIPE, "EOF"),
        (ERROR_BAD_EXE_FORMAT, "EFTYPE"),
        (ERROR_META_EXPANSION_TOO_LONG, "E2BIG"),
    ] {
        assert_eq!(named(code), Some(libuv), "{code}");
    }
}

#[test]
fn a_winsock_code_is_translated_as_a_win32_one_is() {
    for (code, libuv) in [
        (WSAEACCES, "EACCES"),
        (WSAEWOULDBLOCK, "EAGAIN"),
        (WSAECONNREFUSED, "ECONNREFUSED"),
        (WSAEINTR, "ECANCELED"),
        (WSAHOST_NOT_FOUND, "ENOENT"),
        (WSAEPFNOSUPPORT, "EINVAL"),
        (WSAESHUTDOWN, "EPIPE"),
    ] {
        assert_eq!(name(code), Some(libuv), "{code}");
    }
}

#[test]
fn a_code_the_table_does_not_hold_is_unnamed() {
    // Libuv's `UV_UNKNOWN`, which Node prints as `UNKNOWN`.
    assert_eq!(named(ERROR_IO_PENDING), None);
    assert_eq!(name(0), None);
    assert_eq!(name(-4058), None, "a libuv code of its own is no system's");
}

#[test]
fn the_table_holds_every_case_of_libuv_once() {
    assert_eq!(
        TRANSLATION.len(),
        100,
        "the cases of uv_translate_sys_error"
    );
    for (index, (code, _)) in TRANSLATION.iter().enumerate() {
        assert!(
            TRANSLATION[index + 1..]
                .iter()
                .all(|(other, _)| other != code),
            "{code} is translated twice"
        );
    }
}

#[test]
fn every_name_it_gives_is_one_libuv_has_words_for() {
    for (code, libuv) in TRANSLATION {
        assert!(super::super::uv_words(libuv).is_some(), "{code}: {libuv}");
    }
}
