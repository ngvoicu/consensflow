//! The record a send writes, and how it looks for the extension's verdict.

use super::*;
use crate::testing::EPOCH_MS;

#[test]
fn a_message_is_a_bounded_record_with_a_drawn_id_and_one_expiry_in_a_file_of_its_own() {
    let mut stage = Stage::admitting();
    assert!(stage.send("worker followup: exact text").is_empty());
    assert_eq!(stage.entropy.take_draws(), [16]);
    // Bytes 0 to 15 of the scripted stream, `(i * 7 + 3) % 256`.
    let id = "m-030a11181f262d343b424950575e656c";
    let names = stage.names(&stage.inbox());
    assert_eq!(names, [format!("{id}.json")], "no temporary left");
    let text = fs::read_to_string(path::join(&[&stage.inbox(), &names[0]])).unwrap();
    assert_eq!(
        text,
        format!(
            r#"{{"id":"{id}","type":"message","launchId":"{LAUNCH}","session":"native-pi-session","text":"worker followup: exact text","expiresAt":{}}}"#,
            EPOCH_MS + 30_000
        ) + "\n"
    );
    assert_eq!(stage.waits(), [10], "the first look at the verdict");
    assert!(
        fs::read_dir(stage.ack()).unwrap().next().is_none(),
        "the folder of the verdicts made, empty"
    );
}

#[test]
fn a_verdict_is_looked_for_every_ten_milliseconds_through_the_grace_and_the_last_one_is_slept_out()
{
    let mut stage = Stage::admitting();
    stage.begin(0, "nobody admits", "native-pi-session", 1, 25);
    assert!(stage.driver.run().is_empty());
    // The expiry is 25 ms on, and the grace 1,000 past it: 1,025 in all.
    let mut waits = vec![stage.waits()[0]];
    let answer = loop {
        let settled = stage.fire(i64::MAX / 2).expect("a timer left");
        if settled.is_empty() {
            waits.push(stage.waits()[0]);
        } else {
            break answered(settled);
        }
    };
    assert_eq!(
        read(&answer),
        (false, false, Some("admission-unknown"), Some("uncertain"))
    );
    assert_eq!(
        stage.time.wall_ms(),
        EPOCH_MS + 1_026,
        "one past the deadline"
    );
    // A hundred and two of ten, the 5 that was left, the 1 after the deadline.
    let tens = vec![10; 102];
    assert_eq!(waits[..102], tens[..]);
    assert_eq!(waits[102..], [5, 1]);
    assert!(
        stage.names(&stage.inbox()).is_empty(),
        "the record nobody took is taken out of the inbox"
    );
}

#[test]
fn a_verdict_written_before_the_last_look_is_taken_and_one_written_after_it_is_missed() {
    // The looks are at every ten milliseconds, then at the deadline (1,025)
    // and one millisecond past it, which reads nothing.
    for (written_at, taken) in [(1_020, true), (1_025, false)] {
        let mut stage = Stage::admitting();
        stage.begin(0, "answered late", "native-pi-session", 1, 25);
        assert!(stage.driver.run().is_empty());
        while stage.time.wall_ms() < EPOCH_MS + written_at {
            assert!(stage.fire(i64::MAX / 2).unwrap().is_empty());
        }
        assert_eq!(stage.time.wall_ms(), EPOCH_MS + written_at);
        stage.acknowledges(&json!({ "admitted": true, "mode": "tui" }));
        let answer = stage.until_answered();
        assert_eq!(answer.ok, taken, "a verdict written at {written_at} ms");
        assert_eq!(answer.ack.is_some(), taken);
    }
}
