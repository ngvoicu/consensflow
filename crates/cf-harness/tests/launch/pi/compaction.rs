//! The size a project chose for its members' windows to compact at (smaller-contexts,
//! SCX-28 to SCX-30, T-1265): Pi has no flag or variable of its own for it, so a Pi
//! member's launch gives it ConsensFlow's own extension, which runs in every Pi window,
//! as `CF_COMPACTION_TOKENS=<size>`. The extension asks Pi to compact at the window's next
//! rest once its context is past the size, and never where the size is at or above the
//! model's window (`hosts/pi-extension/consensflow-delivery.mjs`, held by
//! `tests/pi-extension.test.mjs`). Nothing of Pi's own settings is written. With no size
//! the launch is as it always was. The chief's launch is given none by the engine, so it is
//! never asked of this adapter.

use super::*;

/// The variable the extension reads the size from.
const SIZE: &str = "CF_COMPACTION_TOKENS";

fn sizes(plan: &Prepared) -> Vec<&str> {
    plan.env
        .iter()
        .filter(|(name, _)| name == SIZE)
        .map(|(_, value)| value.as_str())
        .collect()
}

/// The names of a launch's variables, in order, but the size's.
fn names(plan: &Prepared) -> Vec<&str> {
    plan.env
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| *name != SIZE)
        .collect()
}

#[test]
fn every_members_launch_gives_the_extension_the_size_and_nothing_else_moves() {
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
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
        let sized = prepare(
            &adapter,
            &Request {
                role,
                compaction: Some(400_000),
                ..Request::default()
            },
        )
        .unwrap();
        assert_eq!(sizes(&sized), ["400000"], "{role}");
        assert_eq!(sizes(&plain), Vec::<&str>::new(), "{role}: none chosen");
        assert_eq!(names(&sized), names(&plain), "{role}: the same variables");
        assert_eq!(
            sized.argv.len(),
            plain.argv.len(),
            "{role}: the same command line"
        );
        assert_eq!(sized.drop_env, plain.drop_env, "{role}");
    }
}

#[test]
fn the_size_is_given_to_a_session_resumed_as_to_one_launched() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let resumed = prepare(
        &adapter,
        &Request {
            resume: Some("cf-1-zeus-0a0b0c0d".to_owned()),
            message: None,
            compaction: Some(250_000),
            ..Request::default()
        },
    )
    .unwrap();
    assert_eq!(
        resumed.native_session.as_deref(),
        Some("cf-1-zeus-0a0b0c0d")
    );
    assert_eq!(sizes(&resumed), ["250000"]);
}
