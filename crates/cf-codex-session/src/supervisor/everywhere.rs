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
    for name in [ROLE_VARIABLE, "OPENAI_API_KEY"] {
        assert!(
            removed.iter().any(|gone| gone.eq_ignore_ascii_case(name)),
            "{name} is not taken out: {removed:?}"
        );
    }
}
