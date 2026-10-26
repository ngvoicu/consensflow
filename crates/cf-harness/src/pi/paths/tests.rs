//! What the `pi paths:` scenarios of the goldens never reach: a bare `~`,
//! `~\` on Windows and elsewhere, an environment with no home, an empty home,
//! and the paths as `path.join` writes them. Each answer is `piSessionDir`'s
//! in Node 26 on macOS, except where it says Node asks `os.homedir()`.

use super::*;

/// What the port says where Node asks `os.homedir()`: the sentence of
/// `shared::record::home`.
const MISSING: &str = "missing home in env";

/// The folder `piSessionDir` names for these variables.
fn sessions(vars: &[(&str, &str)]) -> Result<String, String> {
    session_dir(&Env::from_vars(vars.iter().copied()))
}

/// `posix`, a path `path.join` wrote, as the system this is built for writes
/// it: with a backslash for each separator on Windows.
fn native(posix: &str) -> String {
    if cfg!(windows) {
        posix.replace('/', "\\")
    } else {
        posix.to_owned()
    }
}

#[test]
fn the_sessions_are_under_dot_pi_in_the_home_unless_a_variable_says_otherwise() {
    let cases: [(&[(&str, &str)], &str); 6] = [
        (&[("HOME", "/home/me")], "/home/me/.pi/agent/sessions"),
        (
            &[("HOME", "/home/me"), ("PI_CODING_AGENT_DIR", "/agent")],
            "/agent/sessions",
        ),
        (
            &[
                ("PI_CODING_AGENT_DIR", "/agent"),
                ("PI_CODING_AGENT_SESSION_DIR", ""),
            ],
            "/agent/sessions",
        ),
        (
            &[("HOME", "/home/me"), ("USERPROFILE", "/profile")],
            "/home/me/.pi/agent/sessions",
        ),
        (
            &[("USERPROFILE", "/profile")],
            "/profile/.pi/agent/sessions",
        ),
        (
            &[
                ("HOME", "/home/me"),
                ("PI_CODING_AGENT_DIR", "/agent"),
                ("PI_CODING_AGENT_SESSION_DIR", "~/s"),
            ],
            "/home/me/s",
        ),
    ];
    for (vars, expected) in cases {
        assert_eq!(sessions(vars).unwrap(), native(expected), "{vars:?}");
    }
    // A path with no tilde is taken as it is written, and never joined.
    for written in ["/s/./t/../u", "relative/s"] {
        assert_eq!(
            sessions(&[
                ("HOME", "/home/me"),
                ("PI_CODING_AGENT_SESSION_DIR", written)
            ])
            .unwrap(),
            written
        );
    }
}

#[test]
fn a_bare_tilde_is_the_home_and_a_tilde_slash_is_under_it() {
    let home = |variable: &str, configured: &str| {
        sessions(&[("HOME", "/home/me"), (variable, configured)]).unwrap()
    };
    // The home as it is written, not joined.
    assert_eq!(home("PI_CODING_AGENT_SESSION_DIR", "~"), "/home/me");
    assert_eq!(
        home("PI_CODING_AGENT_DIR", "~"),
        native("/home/me/sessions")
    );
    assert_eq!(
        home("PI_CODING_AGENT_DIR", "~/"),
        native("/home/me/sessions")
    );
    assert_eq!(
        home("PI_CODING_AGENT_DIR", "~/a/../b"),
        native("/home/me/b/sessions")
    );
    // A tilde that is not the home's is a name.
    assert_eq!(home("PI_CODING_AGENT_DIR", "~x"), native("~x/sessions"));
    assert_eq!(
        home("PI_CODING_AGENT_DIR", "~me/agent"),
        native("~me/agent/sessions")
    );
    // The home and the path under it are joined as one, slashes at their ends or not.
    assert_eq!(
        sessions(&[("HOME", "/home/me/"), ("PI_CODING_AGENT_DIR", "~/agent/")]).unwrap(),
        native("/home/me/agent/sessions")
    );
}

#[test]
fn a_tilde_backslash_is_the_home_on_windows_alone() {
    let under = |variable: &str, configured: &str| {
        sessions(&[("HOME", "/home/me"), (variable, configured)]).unwrap()
    };
    if cfg!(windows) {
        assert_eq!(
            under("PI_CODING_AGENT_SESSION_DIR", "~\\s"),
            "\\home\\me\\s"
        );
        assert_eq!(
            under("PI_CODING_AGENT_DIR", "~\\agent"),
            "\\home\\me\\agent\\sessions"
        );
    } else {
        // Node on macOS: "~\\s" and "~\\agent/sessions".
        assert_eq!(under("PI_CODING_AGENT_SESSION_DIR", "~\\s"), "~\\s");
        assert_eq!(
            under("PI_CODING_AGENT_DIR", "~\\agent"),
            "~\\agent/sessions"
        );
    }
}

#[test]
fn an_environment_with_no_home_fails_where_the_home_is_read_and_nowhere_else() {
    // Node asks `os.homedir()` for these, and so answers.
    for vars in [
        &[][..],
        &[("PI_CODING_AGENT_DIR", "~/agent")],
        &[("PI_CODING_AGENT_DIR", "~")],
        &[("PI_CODING_AGENT_SESSION_DIR", "~/s")],
        &[("PI_CODING_AGENT_SESSION_DIR", "~")],
        &[("PI_CODING_AGENT_DIR", "")],
    ] {
        assert_eq!(sessions(vars).unwrap_err(), MISSING, "{vars:?}");
    }
    // Nothing asks for a home when no path is of it.
    assert_eq!(
        sessions(&[("PI_CODING_AGENT_SESSION_DIR", "/s")]).unwrap(),
        "/s"
    );
    assert_eq!(
        sessions(&[("PI_CODING_AGENT_DIR", "/agent")]).unwrap(),
        native("/agent/sessions")
    );
    let elsewhere = Env::from_vars([("PI_CODING_AGENT_SESSION_DIR", "/no/such/folder")]);
    assert_eq!(transcript("s", &elsewhere), Ok(None));
    assert_eq!(transcript("s", &Env::default()), Err(MISSING.to_owned()));
}

#[test]
fn an_empty_home_is_a_home_and_a_userprofile_waits_for_home_to_be_unset() {
    // Node: { HOME: '', USERPROFILE: '/profile' } is ".pi/agent/sessions", of the working folder.
    assert_eq!(
        sessions(&[("HOME", ""), ("USERPROFILE", "/profile")]).unwrap(),
        native(".pi/agent/sessions")
    );
    assert_eq!(
        sessions(&[("HOME", ""), ("PI_CODING_AGENT_DIR", "~/agent")]).unwrap(),
        native("agent/sessions")
    );
    assert_eq!(
        sessions(&[("HOME", ""), ("PI_CODING_AGENT_SESSION_DIR", "~")]).unwrap(),
        ""
    );
}

#[test]
fn a_session_is_the_first_file_whose_name_holds_it_and_none_when_there_is_none() {
    let home = tempfile::tempdir().unwrap();
    let project = home
        .path()
        .join(".pi")
        .join("agent")
        .join("sessions")
        .join("project");
    std::fs::create_dir_all(&project).unwrap();
    let hazy = project.join("2026-08-24T18-00-00-000Z_hazy-ridge.jsonl");
    let calm = project.join("2026-08-24T18-00-00-000Z_calm-lake.jsonl");
    std::fs::write(&hazy, "").unwrap();
    std::fs::write(&calm, "").unwrap();
    let env = Env::from_vars([("HOME", home.path().to_str().unwrap())]);
    assert_eq!(transcript("hazy-ridge", &env), Ok(Some(hazy)));
    assert_eq!(transcript("no-such-session", &env), Ok(None));
    // A name that holds the session is enough: part of one is a match.
    assert_eq!(transcript("lake", &env), Ok(Some(calm)));
}

#[test]
fn the_specmint_variant_the_human_installed_is_found_in_pis_own_skills_then_the_agents_and_nowhere_else(
) {
    use crate::shared::specmint::testing::{is_held_to, Fake};
    is_held_to(
        &|env, variant| global_specmint(env, variant),
        &[],
        &[".pi/agent/skills", ".agents/skills"],
        &[
            ".claude/skills",
            ".codex/skills",
            ".config/opencode/skills",
            ".config/devin/skills",
            ".pi/skills",
            ".pi/agent/sessions",
        ],
    );
    // `PI_CODING_AGENT_DIR` moves Pi's own folder, and the agents' stays where it is.
    is_held_to(
        &|env, variant| global_specmint(env, variant),
        &[("PI_CODING_AGENT_DIR", "agent")],
        &["agent/skills", ".agents/skills"],
        &[".pi/agent/skills"],
    );
    // A `~` in it is the home, as it is for Pi.
    let fake = Fake::new();
    let found = fake.install("mine/skills", "specmint-core", "specmint-core");
    let env = Env::from_vars([
        ("HOME", fake.at("")),
        ("PI_CODING_AGENT_DIR", "~/mine".to_owned()),
    ]);
    assert_eq!(global_specmint(&env, Variant::Core), Some(found));
}

/// A home whose Pi settings are `settings`, and what the sessions folder is
/// then, with `vars` beside the home.
fn sessions_set(
    settings: &str,
    vars: &[(&str, &str)],
) -> (tempfile::TempDir, Result<String, String>) {
    let home = tempfile::tempdir().unwrap();
    let agent = home.path().join(".pi").join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(agent.join("settings.json"), settings).unwrap();
    let mut all = vec![("HOME", home.path().to_str().unwrap())];
    all.extend_from_slice(vars);
    let found = sessions(&all);
    (home, found)
}

#[test]
fn the_sessions_are_where_the_user_s_own_settings_put_them_unless_the_variable_says_otherwise() {
    // T-549: Pi writes a member's session where `sessionDir` says, and the
    // board read it nowhere.
    // A whole path, on every system (`/elsewhere` has no drive on Windows).
    let elsewhere = tempfile::tempdir().unwrap();
    let whole = elsewhere.path().to_str().unwrap();
    let settings = serde_json::json!({ "sessionDir": whole }).to_string();
    let (_home, found) = sessions_set(&settings, &[]);
    assert_eq!(found.unwrap(), whole);
    let (home, found) = sessions_set(r#"{"sessionDir": "~/kept"}"#, &[]);
    assert_eq!(
        found.unwrap(),
        path::join(&[home.path().to_str().unwrap(), "kept"])
    );
    let (_home, found) = sessions_set(&settings, &[("PI_CODING_AGENT_SESSION_DIR", "/variable")]);
    assert_eq!(found.unwrap(), "/variable");
    // A relative folder is Pi's working folder's, which a look does not know;
    // no key, an empty one, and a file that is no JSON say nothing.
    for settings in [
        r#"{"sessionDir": "relative/s"}"#,
        r#"{"sessionDir": ""}"#,
        r#"{"theme": "dark"}"#,
        "not json",
    ] {
        let (home, found) = sessions_set(settings, &[]);
        let default = path::join(&[home.path().to_str().unwrap(), ".pi", "agent", "sessions"]);
        assert_eq!(found.unwrap(), default, "{settings}");
    }
}

#[test]
fn a_session_is_found_in_the_folder_the_user_s_settings_name() {
    let home = tempfile::tempdir().unwrap();
    let agent = home.path().join(".pi").join("agent");
    let elsewhere = home.path().join("elsewhere");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let settings = serde_json::json!({ "sessionDir": elsewhere.to_str().unwrap() });
    std::fs::write(agent.join("settings.json"), settings.to_string()).unwrap();
    let file = elsewhere.join("2026-10-25T09-00-00-000Z_cf-1-zeus-0a1b2c3d.jsonl");
    std::fs::write(&file, "").unwrap();
    let env = Env::from_vars([("HOME", home.path().to_str().unwrap())]);
    assert_eq!(transcript("cf-1-zeus-0a1b2c3d", &env), Ok(Some(file)));
}

#[test]
fn the_file_pi_appends_by_itself_is_the_project_s_then_the_user_s_and_none_without_either() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("project");
    let agent = home.path().join(".pi").join("agent");
    std::fs::create_dir_all(project.join(".pi")).unwrap();
    std::fs::create_dir_all(&agent).unwrap();
    let env = Env::from_vars([("HOME", home.path().to_str().unwrap())]);
    let directory = project.to_str().unwrap();
    assert_eq!(append_system_prompt(&env, directory), None);
    let user = agent.join("APPEND_SYSTEM.md");
    std::fs::write(&user, "mine").unwrap();
    assert_eq!(
        append_system_prompt(&env, directory),
        Some(user.to_string_lossy().into_owned())
    );
    let own = project.join(".pi").join("APPEND_SYSTEM.md");
    std::fs::write(&own, "the project's").unwrap();
    assert_eq!(
        append_system_prompt(&env, directory),
        Some(own.to_string_lossy().into_owned())
    );
    // `PI_CODING_AGENT_DIR` moves the user's.
    std::fs::remove_file(&own).unwrap();
    let moved = home.path().join("moved");
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::write(moved.join("APPEND_SYSTEM.md"), "moved").unwrap();
    let env = Env::from_vars([
        ("HOME", home.path().to_str().unwrap()),
        ("PI_CODING_AGENT_DIR", moved.to_str().unwrap()),
    ]);
    assert_eq!(
        append_system_prompt(&env, directory),
        Some(
            moved
                .join("APPEND_SYSTEM.md")
                .to_string_lossy()
                .into_owned()
        )
    );
}
