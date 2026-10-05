//! What a look makes of the session file and the clock, where the goldens'
//! scenarios never reach: the quiet to the millisecond, a session with no
//! file and no home, and a reading that is new each look.

use super::*;

#[test]
fn a_turn_that_ended_is_settled_once_the_file_has_been_quiet_a_hundred_and_twenty_seconds() {
    let steps = |stop: &str| vec![header(), user("u1", "go"), assistant("a1", stop, "done")];
    for stop in ["stop", "aborted"] {
        let mut stage = Stage::with(&steps(stop));
        // Written exactly then: 120,000 milliseconds is enough, 119,999 is not.
        assert_eq!(stage.at(119_999).settlement, Settlement::InFlight, "{stop}");
        let record = stage.at(120_000);
        assert!(!record.in_flight, "{stop}");
        assert_eq!(record.settlement, Settlement::Settled, "{stop}");
        // Written half a millisecond later: 119,999.5 is not enough, and a
        // clock told in whole milliseconds must not round it up.
        stage.written_at(500_000);
        assert_eq!(stage.at(120_000).settlement, Settlement::InFlight, "{stop}");
        assert_eq!(stage.at(120_001).settlement, Settlement::Settled, "{stop}");
    }
    // An error is not settled by waiting; nor is a step that stopped with a call open.
    let mut stage = Stage::with(&steps("error"));
    let record = stage.at(1_000_000);
    assert!(!record.in_flight);
    assert_eq!(record.settlement, Settlement::Unknown);
    let mut stage = Stage::with(&[
        header(),
        user("u1", "go"),
        calling("a1", &json!("c1")),
        assistant("a2", "stop", "done"),
    ]);
    assert_eq!(stage.at(1_000_000).settlement, Settlement::InFlight);
    // A file written after the look's clock is not quiet.
    let mut stage = Stage::with(&steps("stop"));
    assert!(stage.at(-5).in_flight);
}

#[test]
fn a_session_with_no_file_and_no_home_is_unknown_and_a_file_that_arrives_is_read() {
    let mut stage = Stage::new();
    assert_eq!(
        reason(&stage.look(0, &Options::default())),
        "unreadable: no pi session hazy-ridge"
    );
    stage.write(&[header(), user("u1", "go")]);
    assert_eq!(stage.at(0).items.len(), 1);
    let mut homeless = reader(SESSION, &Env::default(), &TimeZone::UTC);
    assert_eq!(
        reason(&homeless.look(&Options::default(), WRITTEN)),
        "unreadable: missing home in env"
    );
}

#[test]
fn every_look_is_a_reading_of_its_own_and_the_reader_works_it_out_anew() {
    let mut stage = Stage::with(&[header(), user("u1", "go"), assistant("a1", "stop", "done")]);
    let first = stage.look(0, &Options::default());
    let second = stage.look(0, &Options::default());
    assert_eq!(first, second);
    assert!(!Arc::ptr_eq(&first, &second));
    // The same transcript and a later clock: another answer.
    assert_ne!(first, stage.look(200_000, &Options::default()));
}
