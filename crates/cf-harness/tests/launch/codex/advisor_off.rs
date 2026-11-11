//! Claude Code's own advisor tool (A5-20): Codex has none, so a Codex member's launch is the
//! launch it always was whether the project has its members run with the tool or not (the
//! engine tells every harness the same; only Claude's adapter acts on it). The strict scan of
//! Codex 0.167.0's binary for the word found none, and `codex features list` lists no such
//! feature.

use super::*;

/// A launch's environment without its session bridge, whose port and token each launch draws.
fn without_bridge(env: &[(String, String)]) -> Vec<(String, String)> {
    env.iter()
        .filter(|(name, _)| name != "CF_CODEX_SESSION_BRIDGE")
        .cloned()
        .collect()
}

/// `words` with the folder of the home that made them read as one place, so that the launches
/// of two homes are compared for what they say and not for where each was made.
fn at_home(words: &[String], root: &str) -> Vec<String> {
    words
        .iter()
        .map(|word| {
            word.replace(root, "<home>")
                .replace(&root.replace('\\', "/"), "<home>")
        })
        .collect()
}

fn env_at_home(env: &[(String, String)], root: &str) -> Vec<(String, String)> {
    env.iter()
        .zip(at_home(
            &env.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(),
            root,
        ))
        .map(|((name, _), value)| (name.clone(), value))
        .collect()
}

#[test]
fn a_codex_members_launch_is_the_same_whether_the_advisor_tool_is_switched_off_or_not() {
    for role in [
        "worker",
        "frontend",
        "architect",
        "researcher",
        "advisor",
        "reviewer",
        "tester",
        "integrator",
        "designer",
    ] {
        let plain_home = Home::new();
        let (plain_adapter, _plain_fakes) = adapter(&plain_home);
        let plain = prepare(
            &plain_adapter,
            &Request {
                role,
                ..Request::default()
            },
        )
        .unwrap();
        let home = Home::new();
        let (adapter, _fakes) = adapter(&home);
        let off = prepare(
            &adapter,
            &Request {
                role,
                advisor_off: true,
                ..Request::default()
            },
        )
        .unwrap();
        assert_eq!(
            at_home(&off.argv, &home.root),
            at_home(&plain.argv, &plain_home.root),
            "{role}: the same command line"
        );
        assert_eq!(
            env_at_home(&without_bridge(&off.env), &home.root),
            env_at_home(&without_bridge(&plain.env), &plain_home.root),
            "{role}: the same environment"
        );
        assert_eq!(off.drop_env, plain.drop_env, "{role}");
        assert_eq!(
            off.native_session
                .as_deref()
                .map(|id| id.replace(&home.root, "<home>")),
            plain
                .native_session
                .as_deref()
                .map(|id| id.replace(&plain_home.root, "<home>")),
            "{role}"
        );
    }
}
