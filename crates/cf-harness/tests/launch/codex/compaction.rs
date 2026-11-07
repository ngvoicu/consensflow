//! The size a project chose for its members' windows to compact at (smaller-contexts,
//! SCX-28 to SCX-30, T-1265): a Codex member's launch gives it Codex as
//! `-c model_auto_compact_token_limit=<size>`, for the launch alone, at launch and on
//! resume. Codex caps it at 90% of the model's window itself
//! (`ModelInfo::auto_compact_token_limit`, codex-rs 0.166.0), so a size above that leaves
//! Codex's own point. With no size the command line is as it always was. The chief's
//! launch is given none by the engine, so it is never asked of this adapter.

use super::*;

/// The flag a size of `size` is given as.
fn limit(size: u32) -> [String; 2] {
    [
        "-c".to_owned(),
        format!("model_auto_compact_token_limit={size}"),
    ]
}

/// `argv` without the two words of the size's flag, where it holds them.
fn without_limit(argv: &[String], size: u32) -> Vec<String> {
    let flag = limit(size);
    let mut kept = Vec::new();
    let mut words = argv.iter().peekable();
    while let Some(word) = words.next() {
        if *word == flag[0] && words.peek().is_some_and(|next| **next == flag[1]) {
            words.next();
            continue;
        }
        kept.push(word.clone());
    }
    kept
}

/// A launch's environment without its session bridge, whose port and token each launch draws.
fn without_bridge(env: &[(String, String)]) -> Vec<(String, String)> {
    env.iter()
        .filter(|(name, _)| name != "CF_CODEX_SESSION_BRIDGE")
        .cloned()
        .collect()
}

#[test]
fn every_members_launch_is_given_the_size_as_codex_s_compaction_limit_and_nothing_else_moves() {
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let home = Home::new();
        let (adapter, _fakes) = adapter(&home);
        let plain = prepare(
            &adapter,
            &Request {
                role,
                ..Request::default()
            },
        )
        .unwrap();
        let sized = prepare(
            &adapter,
            &Request {
                role,
                compaction: Some(400_000),
                ..Request::default()
            },
        )
        .unwrap();
        assert!(
            sized.argv.windows(2).any(|pair| pair == limit(400_000)),
            "{role}: {:?}",
            sized.argv
        );
        assert_eq!(
            sized
                .argv
                .iter()
                .filter(|word| word.starts_with("model_auto_compact_token_limit"))
                .count(),
            1,
            "{role}: once"
        );
        assert_eq!(without_limit(&sized.argv, 400_000), plain.argv, "{role}");
        assert_eq!(
            without_bridge(&sized.env),
            without_bridge(&plain.env),
            "{role}: the same environment"
        );
        assert!(
            !plain
                .argv
                .iter()
                .any(|word| word.contains("model_auto_compact_token_limit")),
            "{role}: a project that chose none gives Codex none"
        );
    }
}

#[test]
fn the_size_is_given_to_a_thread_resumed_as_to_one_launched() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let resumed = prepare(
        &adapter,
        &Request {
            compaction: Some(250_000),
            ..Request::resuming(THREAD)
        },
    )
    .unwrap();
    assert_eq!(resumed.native_session.as_deref(), Some(THREAD));
    let at = resumed
        .argv
        .iter()
        .position(|word| word == "resume")
        .expect("it resumes");
    // Codex's own option, before its `resume` command, as every `-c` of the window is.
    let given = resumed
        .argv
        .windows(2)
        .position(|pair| pair == limit(250_000))
        .expect("the size is given");
    assert!(given < at, "{:?}", resumed.argv);
    let plain = prepare(&adapter, &Request::resuming(THREAD)).unwrap();
    assert_eq!(without_limit(&resumed.argv, 250_000), plain.argv);
}
