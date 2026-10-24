//! A break is recorded when it happens, once: at the first look that finds it,
//! however many looks find it after.

use cf_evals::long_run::invariants::{Break, Evidence, Keeper, INVARIANTS};

use crate::scene::{ms, Scene};

#[test]
fn a_break_is_recorded_once_at_the_first_look_that_finds_it() {
    let scene = Scene::new(&[]);
    let mut keeper = Keeper::default();
    let first = scene.look(ms(1, 0));
    let found = || {
        vec![Break::new(
            3,
            "3:m-132",
            "T-14's close note m-132 came while T-14 was still working",
        )]
    };
    let kept = keeper.keep(&first, found());
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].at, ms(1, 0));
    let later = scene.look(ms(2, 0));
    assert!(keeper.keep(&later, found()).is_empty());
    assert_eq!(keeper.breaks.len(), 1);
    assert!(keeper.broke(3) && !keeper.broke(2));
    assert_eq!(kept[0].name(), "no work is ended under a task at work");
    assert_eq!(kept[0].to_value()["at"], "2026-10-15T10:01:00.000Z");
}

#[test]
fn a_break_of_a_moment_is_recorded_only_when_the_next_look_finds_it_too() {
    let scene = Scene::new(&[]);
    let mut keeper = Keeper::default();
    let moment = || vec![Break::new(6, "6:gone:p1-x-1:1", "gone").twice()];
    assert!(keeper.keep(&scene.look(ms(1, 0)), moment()).is_empty());
    // The next look did not find it: it was a moment, and is forgotten.
    assert!(keeper.keep(&scene.look(ms(1, 10)), Vec::new()).is_empty());
    assert!(keeper.keep(&scene.look(ms(1, 20)), moment()).is_empty());
    let kept = keeper.keep(&scene.look(ms(1, 30)), moment());
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].at, ms(1, 30));
}

#[test]
fn a_gap_a_later_look_fills_is_said_to_have_broken_where_its_check_broke() {
    // Both reviewers': m-5's turn and m-6's were not in their worker's record at the
    // first look; a later look reads both, and its check finds m-5's job ran on past the
    // result (recorded by the look itself) and m-6 held.
    use cf_evals::long_run::invariants::{Gap, Unjudged};
    use cf_evals::long_run::mask::Names;
    let scene = Scene::new(&[]);
    let mut keeper = Keeper::default();
    let open = ["2:m-5", "2:m-6"]
        .map(|key| Unjudged::new(2, key, "its turn is not in the record").gap(Gap::Again));
    keeper.keep_unjudged(ms(1, 0), open.to_vec(), &Names::default());
    let later = scene.look(ms(2, 0));
    keeper.keep(
        &later,
        vec![Break::new(
            2,
            "2:m-5",
            "T-4 was done by m-5 while a job still ran",
        )],
    );
    let filled = keeper.fill(&later, &[]);
    let broke: Vec<(&str, bool)> = filled
        .iter()
        .map(|filled| (filled.open.key.as_str(), filled.broke))
        .collect();
    assert_eq!(broke, [("2:m-5", true), ("2:m-6", false)]);
    assert_eq!(keeper.breaks.len(), 1, "recorded once, by its check");
}

#[test]
fn every_invariant_has_a_name_and_the_shape_it_was_made_of() {
    let numbers: Vec<u8> = INVARIANTS.iter().map(|(number, ..)| *number).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let shapes: Vec<&str> = INVARIANTS.iter().map(|(.., shape)| *shape).collect();
    assert_eq!(
        shapes,
        ["A1", "A2", "A3", "A4", "A5", "B2", "B5", "B1", "end", "play"]
    );
}

#[test]
fn what_looks_like_a_key_or_a_token_is_hidden_before_a_break_is_kept() {
    // A token printed on a screen just before a dialog, a password quoted whole in a
    // record, a command line with a key: kept and said with each hidden.
    let scene = Scene::new(&[]);
    let mut keeper = Keeper::default();
    let found = Break::new(
        8,
        "8:dialog:long-worker:1",
        "a dialog has waited in @long-worker's window, OPENAI_API_KEY=abcd1234efgh on it",
    )
    .with(Evidence {
        rows: vec![
            "7777 node deploy.mjs --token ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_owned(),
        ],
        screen: vec![
            "$ export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789".to_owned(),
            "$ export DB_PASSWORD=".to_owned(),
            "\"correct horse battery staple\"".to_owned(),
            "Allow this edit? (y/n)".to_owned(),
        ],
        record: vec![r#"{"password": "correct horse battery staple"}"#.to_owned()],
    });
    let kept = keeper.keep(&scene.look(ms(1, 0)), vec![found]);
    let kept = &kept[0];
    assert_eq!(
        kept.said,
        "a dialog has waited in @long-worker's window, OPENAI_API_KEY=[masked] on it"
    );
    assert_eq!(
        kept.evidence.rows,
        ["7777 node deploy.mjs --token [masked]"]
    );
    assert_eq!(
        kept.evidence.screen,
        [
            "$ export GITHUB_TOKEN=[masked]",
            "$ export DB_PASSWORD=",
            "\"[masked]\"",
            "Allow this edit? (y/n)"
        ]
    );
    assert_eq!(kept.evidence.record, [r#"{"password": "[masked]"}"#]);
    assert_eq!(keeper.breaks[0], *kept, "kept as said");
}
