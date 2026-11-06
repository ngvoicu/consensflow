//! The size a project chose for its members' windows to compact at (smaller-contexts,
//! SCX-28 to SCX-30, T-1265): a Devin member's launch gives it Devin as
//! `DEVIN_COMPACTION_THRESHOLDS=<size>,<size>` in the window's environment, the variable of
//! Devin's own `--compaction-thresholds` (a summary begins at the first, and no step goes past
//! the second until it is in), for the launch alone, at launch and on resume. Devin says it
//! ignores thresholds a model cannot take ("Ignoring invalid compaction thresholds for model
//! … max_tokens=", 3000.11.234). With no size the launch is as it always was. The chief's
//! launch is given none by the engine, so it is never asked of this adapter.

use cf_harness::contract::Prepared;

use super::fixtures::*;

/// The variable a launch gives Devin its thresholds by.
const THRESHOLDS: &str = "DEVIN_COMPACTION_THRESHOLDS";

fn thresholds(plan: &Prepared) -> Vec<&str> {
    plan.env
        .iter()
        .filter(|(name, _)| name == THRESHOLDS)
        .map(|(_, value)| value.as_str())
        .collect()
}

/// A second launch's id, whose folder is its own: a launch's files are made once.
const SECOND: &str = "3d4e5f60-7182-4394-8dbe-2f3a4b5c6d7e";

/// `words` as the first launch would have said them: the second's id read as the first's.
fn as_first(words: impl IntoIterator<Item = String>) -> Vec<String> {
    words
        .into_iter()
        .map(|word| word.replace(SECOND, LAUNCH))
        .collect()
}

fn without_thresholds(plan: &Prepared) -> Vec<(String, String)> {
    plan.env
        .iter()
        .filter(|(name, _)| name != THRESHOLDS)
        .map(|(name, value)| (name.clone(), value.replace(SECOND, LAUNCH)))
        .collect()
}

#[test]
fn every_members_launch_is_given_the_size_as_devin_s_thresholds_and_nothing_else_moves() {
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let home = Home::new();
        let adapter = home.adapter();
        let plain = prepare(
            &adapter,
            &Request {
                role,
                ..Request::default()
            },
        )
        .unwrap();
        let sized = prepare_as(
            &adapter,
            &Request {
                role,
                compaction: Some(400_000),
                ..Request::default()
            },
            SECOND,
        )
        .unwrap();
        assert_eq!(thresholds(&sized), ["400000,400000"], "{role}");
        assert_eq!(
            thresholds(&plain),
            Vec::<&str>::new(),
            "{role}: none chosen"
        );
        assert_eq!(
            as_first(sized.argv.clone()),
            plain.argv,
            "{role}: the same command line"
        );
        assert_eq!(without_thresholds(&sized), plain.env, "{role}");
        assert_eq!(sized.drop_env, plain.drop_env, "{role}");
    }
}

#[test]
fn the_size_is_given_to_a_session_resumed_as_to_one_launched() {
    let home = Home::new();
    let adapter = home.adapter();
    let resumed = prepare(
        &adapter,
        &Request {
            compaction: Some(250_000),
            ..Request::resumed("mild-coin")
        },
    )
    .unwrap();
    assert_eq!(resumed.native_session.as_deref(), Some("mild-coin"));
    assert_eq!(thresholds(&resumed), ["250000,250000"]);
}
