use super::*;
use crate::roster::testing::{file_with, sentence};
use serde_json::json;
use tempfile::tempdir;

fn refused(path: &Path) -> Refusal {
    read_roster(path).unwrap_err()
}

#[test]
fn the_roster_is_agents_json_in_the_configured_home() {
    let env = Env::from_vars([
        ("CONSENSFLOW_HOME", "/work/consensflow"),
        ("HOME", "/home/me"),
    ]);
    assert_eq!(
        roster_path(&env),
        Some(Path::new("/work/consensflow").join("agents.json"))
    );
}

#[test]
fn without_a_configured_home_it_is_in_the_dot_consensflow_of_the_users_home() {
    let env = Env::from_vars([("HOME", "/home/me")]);
    assert_eq!(
        roster_path(&env),
        Some(
            Path::new("/home/me")
                .join(".consensflow")
                .join("agents.json")
        )
    );
}

#[test]
fn with_no_home_at_all_the_environment_names_no_path() {
    assert_eq!(roster_path(&Env::default()), None);
}

#[test]
fn the_path_in_the_sentence_is_the_joined_path_with_the_systems_separator() {
    let home = tempdir().unwrap();
    fs::create_dir(home.path().join("cf")).unwrap();
    let env = Env::from_vars([("CONSENSFLOW_HOME", home.path().join("cf"))]);
    let path = roster_path(&env).unwrap();
    fs::write(&path, "null").unwrap();
    let message = refused(&path).message;
    let separator = std::path::MAIN_SEPARATOR;
    assert!(
        message.contains(&format!("{separator}cf{separator}agents.json is not")),
        "{message}"
    );
    assert_eq!(message, sentence(&path, "is not an agents file"));
}

#[test]
fn a_missing_file_is_an_empty_roster_and_reading_creates_nothing() {
    let home = tempdir().unwrap();
    assert_eq!(read_roster(&home.path().join("agents.json")).unwrap(), None);
    assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn a_missing_folder_is_a_missing_file_and_is_not_made() {
    let home = tempdir().unwrap();
    let path = home.path().join("consensflow").join("agents.json");
    assert_eq!(read_roster(&path).unwrap(), None);
    assert!(!home.path().join("consensflow").exists());
}

#[test]
fn a_directory_at_the_path_cannot_be_read_and_says_eisdir_on_every_system() {
    let home = tempdir().unwrap();
    let path = home.path().join("agents.json");
    fs::create_dir(&path).unwrap();
    let refusal = refused(&path);
    assert_eq!(refusal.message, sentence(&path, "cannot be read (EISDIR)"));
    assert_eq!(
        (refusal.code, refusal.status),
        ("agents-file-unreadable", 400)
    );
    assert!(path.is_dir(), "it is left as it is");
}

#[test]
fn invalid_json_is_said_with_no_cause_and_the_file_is_left_alone() {
    let broken = br#"{ "agents": [1,] }"#;
    let (_home, path) = file_with(broken);
    let refusal = refused(&path);
    assert_eq!(refusal.message, sentence(&path, "is not valid JSON"));
    assert_eq!(
        (refusal.code, refusal.status),
        ("agents-file-unreadable", 400)
    );
    assert_eq!(fs::read(&path).unwrap(), broken);
}

#[test]
fn what_is_no_json_is_not_valid_json_an_empty_file_included() {
    for text in [
        "",
        " ",
        "{",
        "{} {}",
        "{}x",
        "{'agents':[]}",
        r#"{"agents":[],}"#,
        "undefined",
    ] {
        let (_home, path) = file_with(text.as_bytes());
        assert_eq!(
            refused(&path).message,
            sentence(&path, "is not valid JSON"),
            "{text:?}"
        );
    }
}

#[test]
fn a_byte_order_mark_makes_the_file_invalid_json_as_node_reads_it() {
    // `readFileSync` keeps the mark, and `JSON.parse` refuses it.
    let (_home, path) = file_with(b"\xEF\xBB\xBF{}");
    assert_eq!(refused(&path).message, sentence(&path, "is not valid JSON"));
    let (_plain_home, plain) = file_with(b"{}");
    assert!(
        read_roster(&plain).unwrap().is_some(),
        "the same without the mark"
    );
}

#[test]
fn json_that_is_no_object_is_no_agents_file() {
    for text in [
        "null",
        "[]",
        r#"[{"id":"nova"}]"#,
        "5",
        r#""agents""#,
        "true",
        " null\n",
    ] {
        let (_home, path) = file_with(text.as_bytes());
        let refusal = refused(&path);
        assert_eq!(
            refusal.message,
            sentence(&path, "is not an agents file"),
            "{text:?}"
        );
        assert_eq!(
            (refusal.code, refusal.status),
            ("agents-file-unreadable", 400)
        );
    }
}

#[test]
fn json_around_an_object_is_still_an_object() {
    let (_home, path) = file_with(b"\n\t {\"agents\": []} \r\n");
    let roster = read_roster(&path).unwrap().unwrap();
    assert_eq!(Value::Object(roster), json!({ "agents": [] }));
}

#[test]
fn bytes_that_are_no_utf8_read_as_replacement_characters() {
    let (_home, path) = file_with(b"{\"agents\":[],\"note\":\"a\xFFb\"}");
    let roster = read_roster(&path).unwrap().unwrap();
    assert_eq!(roster["note"], json!("a\u{FFFD}b"));
}

#[test]
fn a_lone_surrogate_escape_reads_as_a_replacement_character() {
    let (_home, path) = file_with(br#"{"note":"cut \ud83d"}"#);
    let roster = read_roster(&path).unwrap().unwrap();
    assert_eq!(roster["note"], json!("cut \u{FFFD}"));
}

#[test]
fn the_object_is_read_in_the_order_javascript_enumerates_its_keys() {
    let (_home, path) = file_with(br#"{"b":1,"2":2,"a":{"z":0,"10":1,"9":2},"1":4}"#);
    let roster = read_roster(&path).unwrap().unwrap();
    assert_eq!(
        Value::Object(roster).to_string(),
        r#"{"1":4,"2":2,"b":1,"a":{"9":2,"10":1,"z":0}}"#
    );
}

#[test]
fn a_key_written_twice_keeps_the_place_of_its_first_and_the_value_of_its_last() {
    let (_home, path) = file_with(br#"{"a":1,"b":2,"a":3}"#);
    let roster = read_roster(&path).unwrap().unwrap();
    assert_eq!(Value::Object(roster).to_string(), r#"{"a":3,"b":2}"#);
}

#[test]
fn json_nested_past_the_limit_serde_json_reads_to_is_refused_as_not_valid_json() {
    // A difference from Node, pinned here for the lead to decide: `JSON.parse`
    // has no limit on how deep JSON nests and reads this file, serde_json
    // stops at 128 levels and the file is said to be no JSON, a sentence Node
    // never had for it. Nothing decided it; no golden holds it.
    let nested = format!("{}{}", "[".repeat(200), "]".repeat(200));
    let (_home, path) = file_with(format!(r#"{{"agents":[],"deep":{nested}}}"#).as_bytes());
    assert_eq!(refused(&path).message, sentence(&path, "is not valid JSON"));
}

#[test]
fn an_error_with_no_errno_says_its_own_words() {
    let error = io::Error::other("the disk is gone");
    assert_eq!(code_of(&error), "the disk is gone");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn a_file_that_may_not_be_read_says_eacces() {
        let (_home, path) = file_with(b"{}");
        mode(&path, 0o000);
        // Root reads what it likes: there is nothing to refuse it.
        if fs::read(&path).is_ok() {
            return;
        }
        assert_eq!(
            refused(&path).message,
            sentence(&path, "cannot be read (EACCES)")
        );
        assert_eq!(fs::metadata(&path).unwrap().len(), 2, "it is left as it is");
    }

    #[test]
    fn a_folder_that_may_not_be_entered_says_eacces() {
        let home = tempdir().unwrap();
        let folder = home.path().join("locked");
        fs::create_dir(&folder).unwrap();
        let path = folder.join("agents.json");
        fs::write(&path, "{}").unwrap();
        mode(&folder, 0o000);
        let refusal = fs::read(&path).is_err().then(|| refused(&path));
        // The folder opens again, so that the temporary home can go.
        mode(&folder, 0o755);
        if let Some(refusal) = refusal {
            assert_eq!(refusal.message, sentence(&path, "cannot be read (EACCES)"));
        }
    }

    #[test]
    fn a_path_through_a_file_says_enotdir() {
        let (_home, file) = file_with(b"x");
        let path = file.join("agents.json");
        assert_eq!(
            refused(&path).message,
            sentence(&path, "cannot be read (ENOTDIR)")
        );
    }

    #[test]
    fn a_link_that_leads_back_to_itself_says_eloop() {
        let home = tempdir().unwrap();
        let path = home.path().join("agents.json");
        symlink("agents.json", &path).unwrap();
        assert_eq!(
            refused(&path).message,
            sentence(&path, "cannot be read (ELOOP)")
        );
    }

    #[test]
    fn a_name_too_long_says_enametoolong() {
        let home = tempdir().unwrap();
        let path = home.path().join("a".repeat(300)).join("agents.json");
        assert_eq!(
            refused(&path).message,
            sentence(&path, "cannot be read (ENAMETOOLONG)")
        );
    }

    #[test]
    fn a_link_to_a_directory_says_eisdir_as_the_directory_does() {
        let home = tempdir().unwrap();
        let directory = home.path().join("elsewhere");
        fs::create_dir(&directory).unwrap();
        let path = home.path().join("agents.json");
        symlink(&directory, &path).unwrap();
        assert_eq!(
            refused(&path).message,
            sentence(&path, "cannot be read (EISDIR)")
        );
    }

    #[test]
    fn a_link_to_nothing_is_a_missing_file() {
        let home = tempdir().unwrap();
        let path = home.path().join("agents.json");
        symlink("nowhere.json", &path).unwrap();
        assert_eq!(read_roster(&path).unwrap(), None);
    }

    #[test]
    fn each_errno_the_table_knows_says_its_own_name() {
        let table = [
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
            (libc::ESTALE, "ESTALE"),
            (libc::EAGAIN, "EAGAIN"),
        ];
        for (errno, name) in table {
            assert_eq!(code_of(&io::Error::from_raw_os_error(errno)), name);
        }
    }

    #[test]
    fn an_errno_the_table_does_not_know_says_the_systems_words() {
        let error = io::Error::from_raw_os_error(libc::ENOSPC);
        let said = code_of(&error);
        assert_eq!(said, error.to_string());
        assert!(
            said.contains(&format!("(os error {})", libc::ENOSPC)),
            "{said}"
        );
    }
}
