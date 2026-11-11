//! Claude Code's prompt suggestion (T-1434, T-1444): Pi 1.5.1 makes no such call (a compaction
//! call and a cache warmer of its own), so a Pi window's launch, the chief's as a member's, is the
//! launch it always was whether the project has the suggestion off or on (the engine tells every
//! harness the same; only Claude's adapter acts on it).

use super::*;

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

/// The launch asked of `role`: the chief's has no first message.
fn asked(role: &'static str) -> Request {
    if role == "chief" {
        Request {
            role,
            message: None,
            ..Request::default()
        }
    } else {
        Request {
            role,
            ..Request::default()
        }
    }
}

#[test]
fn a_pi_windows_launch_is_the_same_whether_the_prompt_suggestion_is_switched_off_or_not() {
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
            env_at_home(&off.env, &home.root),
            env_at_home(&plain.env, &plain_home.root),
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
