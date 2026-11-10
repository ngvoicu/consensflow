//! Claude Code's own advisor tool (A5-20): Devin has none (a strict scan of 3000.12.43's
//! binary for the word found none), so a Devin member's launch is the launch it always was
//! whether the project has its members run with the tool or not (the engine tells every
//! harness the same; only Claude's adapter acts on it).

use super::fixtures::*;

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
fn a_devin_members_launch_is_the_same_whether_the_advisor_tool_is_switched_off_or_not() {
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let plain_home = Home::new();
        let plain = prepare(
            &plain_home.adapter(),
            &Request {
                role,
                ..Request::default()
            },
        )
        .unwrap();
        let home = Home::new();
        let off = prepare(
            &home.adapter(),
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
