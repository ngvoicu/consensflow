//! Codex's launch, whatever Claude Code's prompt suggestion is (T-1434, T-1444): Codex has no
//! such call that was found (0.167.0 has a recap while the terminal is unfocused, which
//! 148 of the owner's rollouts since 2026-11-02 do not hold), so a Codex window's launch, the
//! chief's as a member's, is the launch it always was whether the project has the suggestion off
//! or on (the engine tells every harness the same; only Claude's adapter acts on it).

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

/// The launch asked of `role`: the chief's is no one's with a model and a first message.
fn asked(role: &'static str) -> Request {
    if role == "chief" {
        Request::chief()
    } else {
        Request {
            role,
            ..Request::default()
        }
    }
}

#[test]
fn a_codex_windows_launch_is_the_same_whether_the_prompt_suggestion_is_switched_off_or_not() {
    for role in [
        "chief", "worker", "advisor", "reviewer", "tester", "designer",
    ] {
        let plain_home = Home::new();
        let (plain_adapter, _plain_fakes) = adapter(&plain_home);
        let plain = prepare(&plain_adapter, &asked(role)).unwrap();
        let home = Home::new();
        let (adapter, _fakes) = adapter(&home);
        let off = prepare(
            &adapter,
            &Request {
                suggestion_off: true,
                ..asked(role)
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
