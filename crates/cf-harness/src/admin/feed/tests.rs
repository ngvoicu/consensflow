//! What the admin asks of a feed and reads from its answer; the recorded
//! goldens (`tests/goldens/admin`) hold each to Node's answer, through
//! Node's own `fetch`.

use super::*;
use crate::testing::{
    finished, BodyEnding, Delivery, Driver, ManualTime, ScriptedNetwork, EPOCH_MS,
};

/// A feed over a network the test scripts, and the clock it is bound by.
fn feed() -> (Rc<Feed>, Rc<ScriptedNetwork>, Rc<ManualTime>) {
    let time = Rc::new(ManualTime::new(EPOCH_MS));
    let network = Rc::new(ScriptedNetwork::default());
    let feed = Rc::new(Feed::new(
        Rc::clone(&time) as Rc<dyn Time>,
        Rc::clone(&network) as Rc<dyn Network>,
    ));
    (feed, network, time)
}

/// The source of a harness installed nowhere in particular.
fn source(format: Format) -> Source {
    Source {
        url: "https://feed.test/latest".to_owned(),
        format,
        distribution: None,
        update: None,
    }
}

/// An answer whose body is `chunks`, ending at its end.
fn answered(status: u16, chunks: &[&str]) -> Delivery {
    Delivery::Answer {
        status,
        chunks: chunks
            .iter()
            .map(|chunk| chunk.as_bytes().to_vec())
            .collect(),
        ending: BodyEnding::Whole,
    }
}

fn ask(feed: &Feed, format: Format) -> Result<String, String> {
    finished(Box::pin(async {
        feed.latest(Harness::Codex, &source(format)).await
    }))
}

#[test]
fn the_request_is_bounded_as_nodes_is() {
    assert_eq!(TIMEOUT, Duration::from_secs(5));
    assert_eq!(MAX_UNITS, 2_000_000);
    assert_eq!(unsuccessful(404), "Release service returned HTTP 404");
    assert_eq!(unsuccessful(500), "Release service returned HTTP 500");
}

#[test]
fn a_body_is_cut_off_once_the_text_read_holds_more_than_the_limit() {
    let mut body = Body::default();
    assert_eq!(body.push(&vec![b'a'; MAX_UNITS]), Ok(()));
    assert_eq!(body.text().len(), MAX_UNITS, "exactly the limit is no more");
    assert_eq!(body.push(b"b"), Err(TOO_BIG));
    let mut whole = Body::default();
    assert_eq!(whole.push(&vec![b'a'; MAX_UNITS + 1]), Err(TOO_BIG));
}

#[test]
fn the_length_is_counted_in_utf16_code_units_as_javascript_counts_a_text() {
    // 'é' is two bytes and one unit; an emoji is four bytes and two units.
    let mut body = Body::default();
    assert_eq!(body.push("é".repeat(MAX_UNITS).as_bytes()), Ok(()));
    assert_eq!(body.push(b"x"), Err(TOO_BIG));
    let mut emoji = Body::default();
    assert_eq!(emoji.push("😀".repeat(MAX_UNITS / 2).as_bytes()), Ok(()));
    assert_eq!(emoji.push(b"x"), Err(TOO_BIG));
    let mut over = Body::default();
    assert_eq!(
        over.push("😀".repeat(MAX_UNITS / 2 + 1).as_bytes()),
        Err(TOO_BIG)
    );
}

#[test]
fn each_chunk_is_decoded_on_its_own_so_a_character_cut_between_two_is_lost() {
    let bytes = "a\u{20AC}b".as_bytes();
    let mut whole = Body::default();
    whole.push(bytes).unwrap();
    assert_eq!(whole.text(), "a\u{20AC}b");
    let mut cut = Body::default();
    cut.push(&bytes[..2]).unwrap();
    cut.push(&bytes[2..]).unwrap();
    assert_eq!(cut.text(), "a\u{FFFD}\u{FFFD}\u{FFFD}b");
}

#[test]
fn a_release_is_read_where_the_format_keeps_it() {
    for (format, text, release) in [
        (Format::Npm, r#"{"name":"x","version":"1.2.3"}"#, "1.2.3"),
        (Format::Cask, r#"{"version":"4.5.6"}"#, "4.5.6"),
        (
            Format::Formula,
            r#"{"versions":{"stable":"7.8.9"}}"#,
            "7.8.9",
        ),
        (Format::Text, "  2.1.280\n", "2.1.280"),
        (Format::Npm, r#"{"version":"1.2.3-beta.1"}"#, "1.2.3-beta.1"),
        (
            Format::Text,
            "claude 1.2.3 (stable)",
            "claude 1.2.3 (stable)",
        ),
        (
            Format::Npm,
            r#"{"version":"1.2.3","version":"2.0.0"}"#,
            "2.0.0",
        ),
    ] {
        assert_eq!(
            release_version(format, text),
            Ok(release.to_owned()),
            "{text}"
        );
    }
}

#[test]
fn an_answer_with_no_release_in_it_says_so() {
    for (format, text) in [
        (Format::Npm, r#"{"name":"x"}"#),
        (Format::Npm, r#"{"version":1}"#),
        (Format::Npm, r#"{"version":null}"#),
        (Format::Npm, r#"{"version":"latest"}"#),
        (Format::Npm, "[]"),
        (Format::Npm, r#""1.2.3""#),
        (Format::Npm, "5"),
        (Format::Npm, "null"),
        (Format::Cask, r#"{"versions":{"stable":"1.2.3"}}"#),
        (Format::Formula, r#"{"version":"1.2.3"}"#),
        (Format::Formula, r#"{"versions":"1.2.3"}"#),
        (Format::Formula, r#"{"versions":{"stable":null}}"#),
        (Format::Formula, r#"{"versions":null}"#),
        (Format::Formula, "null"),
        (Format::Text, ""),
        (Format::Text, "  \n"),
        (Format::Text, "no release here"),
    ] {
        assert_eq!(
            release_version(format, text),
            Err(UNAVAILABLE.to_owned()),
            "{format:?} {text}"
        );
    }
}

#[test]
fn an_answer_that_is_no_json_where_json_is_read_says_so_in_this_crates_own_words() {
    for text in [
        "",
        "<html>",
        "{\"version\":",
        "\u{FEFF}{\"version\":\"1.2.3\"}",
        "{'version':1}",
    ] {
        assert_eq!(
            release_version(Format::Npm, text),
            Err(NOT_JSON.to_owned()),
            "{text:?}"
        );
    }
    // Plain text is never read as JSON.
    assert_eq!(
        release_version(Format::Text, "<html>"),
        Err(UNAVAILABLE.to_owned())
    );
}

#[test]
fn what_fetch_said_of_a_connection_that_failed_is_kept_word_for_word() {
    assert_eq!(FETCH_FAILED, "fetch failed");
    assert_eq!(TERMINATED, "terminated");
    assert_eq!(TIMED_OUT, "The operation was aborted due to timeout");
}

#[test]
fn a_feed_is_asked_once_without_following_a_redirect_within_five_seconds() {
    let (feed, network, _) = feed();
    network.serve(answered(200, &[r#"{"version":"#, r#""1.2.3"}"#]));
    assert_eq!(ask(&feed, Format::Npm), Ok("1.2.3".to_owned()));
    assert_eq!(
        network.take_asked(),
        [("https://feed.test/latest".to_owned(), TIMEOUT, false)]
    );
    assert_eq!(network.unused(), 0);
}

#[test]
fn a_status_that_is_no_success_ends_the_ask_before_the_body_is_read() {
    for status in [300, 304, 305, 400, 404, 500, 503] {
        let (feed, network, _) = feed();
        network.serve(answered(status, &[r#"{"version":"1.2.3"}"#]));
        assert_eq!(
            ask(&feed, Format::Npm),
            Err(format!("Release service returned HTTP {status}"))
        );
        assert_eq!(network.chunks_read(), 0, "{status}");
    }
    let (feed, network, _) = feed();
    network.serve(answered(204, &[]));
    assert_eq!(ask(&feed, Format::Npm), Err(NOT_JSON.to_owned()));
    network.serve(answered(299, &["1.2.3"]));
    assert_eq!(ask(&feed, Format::Text), Ok("1.2.3".to_owned()));
}

#[test]
fn a_redirect_is_refused_in_fetch_s_words_whatever_it_carries_and_no_other_status_is() {
    assert_eq!(REDIRECTS, [301, 302, 303, 307, 308]);
    for status in REDIRECTS {
        let (feed, network, _) = feed();
        network.serve(answered(status, &[r#"{"version":"1.2.3"}"#]));
        assert_eq!(ask(&feed, Format::Npm), Err("fetch failed".to_owned()));
        assert_eq!(network.chunks_read(), 0, "{status}");
    }
}

#[test]
fn a_body_past_the_limit_ends_the_ask_and_is_read_no_further() {
    let (feed, network, _) = feed();
    let half = "x".repeat(MAX_UNITS / 2 + 1);
    network.serve(answered(200, &[&half, &half, "never read"]));
    assert_eq!(ask(&feed, Format::Npm), Err(TOO_BIG.to_owned()));
    assert_eq!(network.chunks_read(), 2);
}

#[test]
fn a_connection_that_fails_or_closes_says_the_networks_words() {
    let (feed, network, _) = feed();
    network.serve(Delivery::Failure(FETCH_FAILED.to_owned()));
    assert_eq!(ask(&feed, Format::Npm), Err("fetch failed".to_owned()));
    network.serve(Delivery::Answer {
        status: 200,
        chunks: vec![br#"{"vers"#.to_vec()],
        ending: BodyEnding::Cut,
    });
    assert_eq!(ask(&feed, Format::Npm), Err("terminated".to_owned()));
}

#[test]
fn the_five_seconds_bound_the_whole_request_the_head_and_the_body_alike() {
    for served in [
        Delivery::Silence,
        Delivery::Answer {
            status: 200,
            chunks: vec![br#"{"vers"#.to_vec()],
            ending: BodyEnding::Never,
        },
    ] {
        let (feed, network, time) = feed();
        network.serve(served);
        let mut driver = Driver::default();
        let asking = Rc::clone(&feed);
        driver.begin(0, async move {
            asking.latest(Harness::Codex, &source(Format::Npm)).await
        });
        assert!(driver.run().is_empty());
        assert_eq!(time.waits(0), [5000], "armed before the request began");
        assert!(!time.fire_next(EPOCH_MS + 4999));
        assert!(time.fire_next(EPOCH_MS + 5000));
        assert_eq!(driver.run(), [(0, Err(TIMED_OUT.to_owned()))]);
    }
}

#[test]
fn an_answer_in_time_leaves_no_timer_armed() {
    let (feed, network, time) = feed();
    network.serve(answered(200, &[r#"{"version":"1.2.3"}"#]));
    assert_eq!(ask(&feed, Format::Npm), Ok("1.2.3".to_owned()));
    assert!(!time.fire_next(i64::MAX), "its timer went with the request");
}
