//! The two readers, held to Node's: what is counted, when a body is refused,
//! and what is made of what came.

use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use futures_util::stream;
use serde_json::json;

use super::*;

/// What Node's readers hold, as numbers: the limits are not the constants
/// they are held to.
const TWO_MEBIBYTES: usize = 2 * 1024 * 1024;
const SIXTY_FOUR_K_UNITS: usize = 64 * 1024;

/// `work`, or none if it has not ended in five seconds: a reader that waits
/// for the rest of a body that never ends fails its test, and does not hang it.
async fn timed<F: Future>(work: F) -> Option<F::Output> {
    tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .ok()
}

fn body(chunks: &[&[u8]]) -> Body {
    Body::new(stream::iter(
        chunks
            .iter()
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .collect::<Vec<_>>(),
    ))
}

/// A body whose chunks come, and then the test learns whether anyone asked for more.
fn body_then_watched(chunks: Vec<Vec<u8>>, asked: &Rc<Cell<bool>>) -> Body {
    let asked = Rc::clone(asked);
    Body::new(
        stream::iter(chunks.into_iter().map(|chunk| Ok(Bytes::from(chunk)))).chain(
            stream::poll_fn(move |_| {
                asked.set(true);
                std::task::Poll::Pending
            }),
        ),
    )
}

fn refusal(failure: Failure) -> (u16, &'static str, String) {
    let Failure::Refused(refusal) = failure else {
        panic!("not a refusal: {failure:?}");
    };
    (refusal.status, refusal.code, refusal.message)
}

#[tokio::test]
async fn no_body_is_an_empty_object() {
    assert_eq!(read_json(&mut Body::empty()).await.unwrap(), Map::new());
    assert_eq!(read_json(&mut body(&[b"", b""])).await.unwrap(), Map::new());
}

#[tokio::test]
async fn chunks_are_one_body_and_the_keys_keep_their_order() {
    let read = read_json(&mut body(&[br#"{"b": 1, "a":"#, br#" [true, null]}"#]))
        .await
        .unwrap();
    let keys: Vec<&String> = read.keys().collect();
    assert_eq!(keys, ["b", "a"]);
    assert_eq!(Value::Object(read), json!({ "b": 1, "a": [true, null] }));
}

#[tokio::test]
async fn what_is_no_one_json_object_is_invalid_json() {
    for text in [
        "[]", "[{}]", "null", "7", "\"text\"", "true", "{", "{} {}", " ", "{'a':1}",
    ] {
        let failure = read_json(&mut body(&[text.as_bytes()])).await.unwrap_err();
        assert_eq!(
            refusal(failure),
            (
                400,
                "invalid-json",
                "the request body must be a JSON object".to_owned()
            ),
            "{text:?}"
        );
    }
}

#[tokio::test]
async fn a_body_of_exactly_two_mebibytes_is_read_and_one_byte_more_is_too_large() {
    let filler = "x".repeat(TWO_MEBIBYTES - r#"{"t":""}"#.len());
    let exact = format!(r#"{{"t":"{filler}"}}"#);
    assert_eq!(exact.len(), TWO_MEBIBYTES);
    let read = read_json(&mut body(&[exact.as_bytes()])).await.unwrap();
    assert_eq!(read["t"].as_str().map(str::len), Some(filler.len()));

    let over = format!(r#"{{"t":"{filler}x"}}"#);
    let failure = read_json(&mut body(&[over.as_bytes()])).await.unwrap_err();
    assert_eq!(
        refusal(failure),
        (
            413,
            "too-large",
            "the request is larger than 2 MB".to_owned()
        )
    );
}

#[tokio::test]
async fn a_body_is_refused_as_the_chunk_that_passes_the_limit_comes_not_when_it_is_all_there() {
    let asked = Rc::new(Cell::new(false));
    let mut come = body_then_watched(
        vec![
            vec![b' '; TWO_MEBIBYTES / 2],
            vec![b' '; TWO_MEBIBYTES / 2 + 1],
        ],
        &asked,
    );
    let failure = timed(read_json(&mut come))
        .await
        .expect("refused as the chunk came, with no wait for the rest")
        .unwrap_err();
    assert_eq!(refusal(failure).1, "too-large");
    assert!(
        !asked.get(),
        "nothing waited for the rest of a body that never ends"
    );
}

#[tokio::test]
async fn a_connection_that_failed_mid_body_is_the_failure_it_said() {
    let mut broken = Body::new(stream::iter(vec![
        Ok(Bytes::from_static(b"{\"a\":")),
        Err(io::Error::other("connection reset")),
    ]));
    assert_eq!(
        read_json(&mut broken).await.unwrap_err(),
        Failure::Internal("connection reset".to_owned())
    );
}

#[tokio::test]
async fn invalid_utf8_in_a_json_body_reads_as_the_replacement_character() {
    let read = read_json(&mut body(&[b"{\"a\":\"caf\xff\"}"]))
        .await
        .unwrap();
    assert_eq!(read["a"], "caf\u{fffd}");
}

#[tokio::test]
async fn a_screens_body_is_its_text_and_empty_when_there_is_none() {
    assert_eq!(read_text(&mut Body::empty()).await.unwrap(), "");
    assert_eq!(
        read_text(&mut body(&[b"{\"name\":", b"\"caf\xc3\xa9\"}"]))
            .await
            .unwrap(),
        "{\"name\":\"caf\u{e9}\"}"
    );
}

#[tokio::test]
async fn a_screens_body_is_counted_in_utf16_units_not_in_bytes() {
    // 32768 emoji are 131072 bytes and 65536 units: the limit, and no more.
    let emoji = "\u{1F600}".repeat(SIXTY_FOUR_K_UNITS / 2);
    assert_eq!(emoji.len(), 2 * SIXTY_FOUR_K_UNITS);
    assert_eq!(
        read_text(&mut body(&[emoji.as_bytes()]))
            .await
            .unwrap()
            .len(),
        emoji.len()
    );
    // One unit more is too much: here an accent, which is a unit and two bytes.
    let over = format!("{emoji}\u{e9}");
    assert_eq!(
        read_text(&mut body(&[over.as_bytes()])).await.unwrap_err(),
        Unread::TooLarge
    );
    // And two bytes that are one unit are not two.
    let plain = "\u{e9}".repeat(SIXTY_FOUR_K_UNITS);
    assert_eq!(
        read_text(&mut body(&[plain.as_bytes()]))
            .await
            .unwrap()
            .chars()
            .count(),
        SIXTY_FOUR_K_UNITS
    );
}

#[tokio::test]
async fn a_screens_body_counts_the_chunks_together_and_stops_at_the_limit() {
    let asked = Rc::new(Cell::new(false));
    let mut come = body_then_watched(
        vec![vec![b'a'; SIXTY_FOUR_K_UNITS - 1], b"bb".to_vec()],
        &asked,
    );
    let unread = timed(read_text(&mut come))
        .await
        .expect("refused as the chunk came, with no wait for the rest");
    assert_eq!(unread.unwrap_err(), Unread::TooLarge);
    assert!(!asked.get());
    assert_eq!(Unread::TooLarge.message(), "body too large");
}

#[tokio::test]
async fn a_character_cut_between_two_chunks_reads_as_replacements_as_node_read_it() {
    // Node decoded each chunk on its own: the two halves of one character are two replacements.
    let cut = "\u{e9}".as_bytes();
    let read = read_text(&mut body(&[&cut[..1], &cut[1..]])).await.unwrap();
    assert_eq!(read, "\u{fffd}\u{fffd}");
}

#[tokio::test]
async fn a_connection_that_failed_in_a_screens_body_says_what_it_said() {
    let mut broken = Body::new(stream::iter(vec![Err(io::Error::other("aborted"))]));
    let unread = read_text(&mut broken).await.unwrap_err();
    assert_eq!(unread, Unread::Broke("aborted".to_owned()));
    assert_eq!(unread.message(), "aborted");
}
