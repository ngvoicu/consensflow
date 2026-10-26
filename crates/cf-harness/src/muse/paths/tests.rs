use std::fs;

use super::*;

fn env_of(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

#[test]
fn the_data_folder_is_xdg_s_and_an_empty_one_is_none_as_muse_reads_it() {
    let at = |vars: &[(&str, &str)]| data(&env_of(vars)).unwrap();
    assert_eq!(
        at(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")]),
        PathBuf::from("/x/muse")
    );
    let default = PathBuf::from(path::join(&["/h", ".local", "share"])).join("muse");
    assert_eq!(at(&[("XDG_DATA_HOME", ""), ("HOME", "/h")]), default);
    assert_eq!(at(&[("HOME", "/h")]), default);
    assert_eq!(at(&[("USERPROFILE", "/h")]), default);
    assert_eq!(data(&env_of(&[])).unwrap_err(), "missing home in env");
    assert_eq!(
        index(&env_of(&[("XDG_DATA_HOME", "/x")])).unwrap(),
        PathBuf::from("/x/muse/session-index.db")
    );
}

#[test]
fn a_session_s_log_is_in_the_folder_named_for_it_under_a_date_and_never_a_child_s() {
    let home = tempfile::tempdir().unwrap();
    let env = env_of(&[("XDG_DATA_HOME", home.path().to_str().unwrap())]);
    let sessions = home.path().join("muse").join("sessions");
    let write = |relative: &str| {
        let file = sessions.join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "{}\n").unwrap();
        file
    };
    assert_eq!(transcript("s1", &env).unwrap(), None, "no sessions folder");
    // A child agent's log, a copy beside it, a folder of no date: none is the session's.
    write("2026/10/26/parent/subagent/s1/session.jsonl");
    write("2026/10/26/s1/session.jsonl.bak");
    write("drafts/10/26/s1/session.jsonl");
    write("2026/10/26/s10/session.jsonl");
    assert_eq!(transcript("s1", &env).unwrap(), None);
    let log = write("2026/10/26/s1/session.jsonl");
    assert_eq!(transcript("s1", &env).unwrap(), Some(log.clone()));
    // A later date's folder of the same id is not read before the first.
    write("2026/11/01/s1/session.jsonl");
    assert_eq!(transcript("s1", &env).unwrap(), Some(log));
    // Its folder is named as the session is written, on a disk that folds case too.
    assert_eq!(transcript("S1", &env).unwrap(), None);
    for odd in ["", ".", "..", "2026/10/26/s1", "a\\b"] {
        assert_eq!(transcript(odd, &env).unwrap(), None, "{odd:?}");
    }
}
