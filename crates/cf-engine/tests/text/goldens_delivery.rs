//! The tables of delivery text in `tests/goldens/text.json`: how a message
//! reads in the window, and the marker that proves it arrived.

use cf_base::text::window_text;
use cf_engine::delivery_text::{delivery_text, marker_of};

use super::support::{assert_same_text, message_of, table};

#[test]
fn every_message_reads_in_the_window_as_node_gave_it() {
    let rows = table("deliveryText");
    let (mut with_text, mut halved) = (0, 0);
    for row in rows {
        let message = message_of(&row["message"]);
        let text = delivery_text(&message, &[]);
        let what = format!(
            "m-{} {} ({} units)",
            message.id,
            message.kind,
            message.body.len()
        );
        assert_same_text(&window_text(&text), row["window"].as_str().unwrap(), &what);
        if let Some(node) = row.get("text") {
            assert_same_text(&text, node.as_str().unwrap(), &what);
            with_text += 1;
        }
        if row.get("halved").is_some() {
            // Node left half of a pair for the window to drop; here it is not there to be dropped.
            assert!(
                !text.contains('\u{FFFD}'),
                "{what}: a half written as U+FFFD"
            );
            halved += 1;
        }
    }
    assert_eq!(rows.len(), 239);
    assert_eq!((with_text, halved), (10, 7));
}

#[test]
fn every_marker_is_the_start_of_the_header_the_message_arrives_under() {
    let rows = table("marker");
    for row in rows {
        let id = row["id"].as_i64().unwrap();
        assert_eq!(marker_of(id), row["marker"].as_str().unwrap(), "m-{id}");
    }
    assert_eq!(rows.len(), 6);
}
