//! The supervisor's parts that are the same on every platform, tested on each (the
//! whole windows of `tests` run shell scripts as Codex, so Unix's): the role read
//! from its file, and what Codex's programs are not told.

use super::*;

/// An environment that names `file` as the role's.
fn naming(file: &Path) -> Env {
    Env::from_vars([(ROLE_VARIABLE, file.as_os_str().to_owned())])
}

#[test]
fn the_role_is_the_text_of_the_file_the_variable_names_and_none_without_the_variable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("developer-instructions.md");
    std::fs::write(&file, "the role \u{e9}\r\n").unwrap();
    assert_eq!(
        role_of(&naming(&file)).unwrap().as_deref(),
        Some("the role \u{e9}\r\n")
    );
    assert!(role_of(&Env::from_vars([("HOME", "/home")]))
        .unwrap()
        .is_none());
}

#[test]
fn a_role_file_that_is_missing_or_empty_or_a_variable_set_to_nothing_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone.md");
    let failed = role_of(&naming(&missing)).unwrap_err().to_string();
    assert!(
        failed.starts_with(&format!(
            "could not read its role from {}: ",
            missing.display()
        )),
        "{failed}"
    );
    let empty = dir.path().join("empty.md");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(
        role_of(&naming(&empty)).unwrap_err().to_string(),
        format!("its role file {} holds nothing", empty.display())
    );
    let unset = Env::from_vars([(ROLE_VARIABLE, "")]);
    assert_eq!(
        role_of(&unset).unwrap_err().to_string(),
        "CF_CODEX_ROLE_FILE names no file"
    );
}

#[test]
fn neither_of_codexs_programs_is_told_where_the_role_is_nor_given_an_openai_key() {
    let program = std::env::current_exe().unwrap();
    let env = Env::from_vars([
        (ROLE_VARIABLE, "/somewhere/developer-instructions.md"),
        (SUBAGENT_VARIABLE, "/somewhere/sub-agents.toml"),
        (SUBAGENT_TYPES_VARIABLE, "default"),
        ("OPENAI_API_KEY", "sk-not-for-codex"),
    ]);
    let plan = Plan {
        env: &env,
        executable: &program,
        bridge: Bridge::parse(
            r#"{"launchId":"launch-1","port":0,"token":"private-launch-token-1234567890"}"#,
        )
        .unwrap(),
        role: None,
        children: None,
        split: Split::default(),
        endpoint: Endpoint::loopback().unwrap(),
        bypass: false,
        startup: STARTUP,
        refusal: Setup::standard().refusal,
    };
    let (command, _) = command(&plan, &[]).unwrap();
    let removed: Vec<_> = command
        .as_std()
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    for name in [
        ROLE_VARIABLE,
        SUBAGENT_VARIABLE,
        SUBAGENT_TYPES_VARIABLE,
        "OPENAI_API_KEY",
    ] {
        assert!(
            removed.iter().any(|gone| gone.eq_ignore_ascii_case(name)),
            "{name} is not taken out: {removed:?}"
        );
    }
}

#[test]
fn the_sub_agents_file_is_the_one_the_variable_names_and_none_without_the_variable() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sub-agents.toml");
    std::fs::write(&file, "developer_instructions = \"Not the worker.\"\n").unwrap();
    let naming = |value: &std::ffi::OsStr| Env::from_vars([(SUBAGENT_VARIABLE, value.to_owned())]);
    // Each of Codex's own types where none is named, in Codex's order.
    assert_eq!(
        children_of(&naming(file.as_os_str())).unwrap(),
        Some(Children {
            file: file.clone(),
            kinds: AGENT_TYPES.iter().collect(),
        })
    );
    // The types named alone: one the owner defines is left out of them.
    let named = |types: &str| {
        let env = Env::from_vars([
            (SUBAGENT_VARIABLE, file.as_os_str().to_owned()),
            (SUBAGENT_TYPES_VARIABLE, types.into()),
        ]);
        children_of(&env).map(|children| {
            children.map(|children| {
                children
                    .kinds
                    .iter()
                    .map(|kind| kind.name)
                    .collect::<Vec<_>>()
            })
        })
    };
    assert_eq!(
        named("default,explorer").unwrap(),
        Some(vec!["default", "explorer"])
    );
    assert_eq!(
        named("").unwrap(),
        Some(vec![]),
        "the owner defines all three"
    );
    assert_eq!(
        named("default,awaiter").unwrap_err().to_string(),
        "CF_CODEX_SUBAGENT_TYPES names awaiter, which is none of Codex's own agent types"
    );
    assert_eq!(
        children_of(&Env::from_vars([("HOME", "/h")])).unwrap(),
        None
    );
    // Set to nothing, empty, or not there: no window, as for a role's file.
    assert_eq!(
        children_of(&naming(std::ffi::OsStr::new("")))
            .unwrap_err()
            .to_string(),
        "CF_CODEX_SUBAGENT_FILE names no file"
    );
    let empty = dir.path().join("empty.toml");
    std::fs::write(&empty, "").unwrap();
    assert!(matches!(
        children_of(&naming(empty.as_os_str())),
        Err(SessionError::NoChildren { .. })
    ));
    assert!(matches!(
        children_of(&naming(dir.path().join("gone.toml").as_os_str())),
        Err(SessionError::Children { .. })
    ));
}

fn failed_wait() -> io::Error {
    io::Error::other("the wait failed")
}

#[test]
fn a_wait_that_failed_is_a_failure_and_no_exit_of_zero() {
    let failure = exit_code(Err(failed_wait())).unwrap_err();
    assert!(matches!(failure, SessionError::Wait(_)), "{failure:?}");
    assert_eq!(
        failure.to_string(),
        "could not wait for Codex's program to end: the wait failed"
    );
    assert_eq!(failure.startup(), StartKind::NotSetUp);
}

#[test]
fn a_poll_of_the_server_that_failed_is_a_failure_and_not_a_server_still_running() {
    let failure = ended_of(Some(Err(failed_wait()))).unwrap_err();
    assert!(matches!(failure, SessionError::Wait(_)), "{failure:?}");
    assert_eq!(ended_of(Some(Ok(None))).unwrap(), None);
    assert_eq!(ended_of(None).unwrap(), None);
}

#[test]
fn a_status_that_was_read_gives_its_code_and_a_signal_gives_zero() {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    #[cfg(unix)]
    let (code_seven, signalled) = (ExitStatus::from_raw(7 << 8), ExitStatus::from_raw(15));
    #[cfg(windows)]
    let (code_seven, signalled) = (ExitStatus::from_raw(7), ExitStatus::from_raw(7));
    assert_eq!(exit_code(Ok(code_seven)).unwrap(), 7);
    #[cfg(unix)]
    assert_eq!(exit_code(Ok(signalled)).unwrap(), 0);
    #[cfg(windows)]
    let _ = signalled;
    assert_eq!(
        ended_of(Some(Ok(Some(code_seven)))).unwrap(),
        Some(code_seven)
    );
}
