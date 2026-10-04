use super::*;
use std::fs;

#[test]
fn a_failure_is_called_what_node_calls_it() {
    let dir = tempfile::tempdir().unwrap();
    let missing = fs::read(dir.path().join("none")).unwrap_err();
    assert_eq!(errno_name(&missing), Some("ENOENT"));
    assert_eq!(error_code(&missing), "ENOENT");
    assert!(is_missing(&missing));
    let directory = fs::read(dir.path()).unwrap_err();
    // Unix opens a folder and refuses to read it; Windows refuses to open
    // it, as access denied: Node's word for that is EPERM.
    let word = errno_name(&directory);
    assert!(word == Some("EISDIR") || word == Some("EPERM"), "{word:?}");
    assert!(!is_missing(&directory));
}

#[test]
fn a_failure_with_no_system_code_is_unnamed_and_node_calls_it_unknown() {
    let error = io::Error::other("no system code");
    assert_eq!(errno_name(&error), None);
    assert_eq!(error_code(&error), "UNKNOWN");
    assert!(!is_missing(&error));
}

#[test]
fn libuv_has_words_for_a_name_of_its_map_and_none_for_another() {
    assert_eq!(uv_words("EACCES"), Some("permission denied"));
    assert_eq!(uv_words("ENOENT"), Some("no such file or directory"));
    assert_eq!(uv_words("EAI_NONAME"), Some("unknown node or service"));
    assert_eq!(uv_words("UNKNOWN"), Some("unknown error"));
    assert_eq!(uv_words("ESTALE"), None);
    assert_eq!(uv_words("eacces"), None);
}

#[test]
fn a_name_libuv_does_not_hold_has_the_words_of_an_unknown_error() {
    assert_eq!(words_of("EACCES"), "permission denied");
    assert_eq!(words_of("ESTALE"), "unknown error");
    assert_eq!(UNKNOWN, ("UNKNOWN", words_of("UNKNOWN")));
}

#[test]
fn no_name_is_in_the_words_table_twice() {
    for (index, (name, _)) in WORDS.iter().enumerate() {
        assert!(
            WORDS[index + 1..].iter().all(|(other, _)| other != name),
            "{name} twice"
        );
    }
}

#[cfg(windows)]
#[test]
fn on_windows_a_name_no_file_can_have_is_missing_as_node_says() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = fs::read(dir.path().join("a<b").join("agents.json")).unwrap_err();
    assert_eq!(errno_name(&invalid), Some("ENOENT"));
    assert!(is_missing(&invalid));
}

#[cfg(unix)]
mod unix {
    use super::*;

    #[test]
    fn an_errno_libuv_names_is_called_by_its_name() {
        for (errno, name) in [
            (libc::ENOENT, "ENOENT"),
            (libc::EACCES, "EACCES"),
            (libc::EPERM, "EPERM"),
            (libc::EISDIR, "EISDIR"),
            (libc::ENOTDIR, "ENOTDIR"),
            (libc::ELOOP, "ELOOP"),
            (libc::ENAMETOOLONG, "ENAMETOOLONG"),
            (libc::EIO, "EIO"),
            (libc::EMFILE, "EMFILE"),
            (libc::ENFILE, "ENFILE"),
            (libc::ENOMEM, "ENOMEM"),
            (libc::EBUSY, "EBUSY"),
            (libc::ENXIO, "ENXIO"),
            (libc::ENODEV, "ENODEV"),
            (libc::EINVAL, "EINVAL"),
            (libc::EOVERFLOW, "EOVERFLOW"),
            (libc::ETIMEDOUT, "ETIMEDOUT"),
            (libc::EAGAIN, "EAGAIN"),
            (libc::ENOSPC, "ENOSPC"),
            (libc::EFBIG, "EFBIG"),
            (libc::EROFS, "EROFS"),
            (libc::EEXIST, "EEXIST"),
            (libc::ENOTEMPTY, "ENOTEMPTY"),
            (libc::EXDEV, "EXDEV"),
            (libc::EHOSTUNREACH, "EHOSTUNREACH"),
        ] {
            assert_eq!(errno_name(&io::Error::from_raw_os_error(errno)), Some(name));
        }
    }

    #[test]
    fn an_errno_libuv_has_no_name_for_is_unnamed_and_node_calls_it_unknown() {
        // Node's `uvErrmapGet` finds nothing for these, so they are `UNKNOWN`.
        for errno in [libc::ESTALE, libc::EDQUOT, libc::EDEADLK, libc::ECHILD] {
            let error = io::Error::from_raw_os_error(errno);
            assert_eq!(errno_name(&error), None, "{errno}");
            assert_eq!(error_code(&error), "UNKNOWN");
        }
    }

    #[test]
    fn every_errno_of_the_table_is_named_once_and_its_name_has_words() {
        for (index, (errno, name)) in SYSTEM.iter().enumerate() {
            assert!(
                SYSTEM[index + 1..].iter().all(|(other, _)| other != errno),
                "{name}'s errno {errno} is named twice"
            );
            assert!(uv_words(name).is_some(), "{name}");
        }
    }
}
