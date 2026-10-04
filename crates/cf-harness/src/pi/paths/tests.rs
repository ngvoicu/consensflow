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
