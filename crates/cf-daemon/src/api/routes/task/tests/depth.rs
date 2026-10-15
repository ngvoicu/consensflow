//! How far back `cf task get T-n --transcript` reaches: the evidence for the field
//! report of 2026-10-15 (batch B, B7), where `--last 400` did not reach the window
//! that overwrote a file. The copy keeps every item (`copy_transcript`), and a
//! request gives the last 50 of them at most, whatever it asks: what came before
//! those is in no answer of this route. What to do about it is the owner's (the
//! brain's `issues/field-reports-2026-10-15.md`).

use super::{get, written_by_zeus};
use crate::api::routes::tests::support::working_task;
use crate::testing::scene;

#[tokio::test]
async fn a_transcript_gives_the_last_fifty_items_however_many_are_asked_for_and_none_before() {
    let scene = scene();
    let number = working_task(&scene);
    written_by_zeus(&scene, 400);
    for last in ["400", "50", "1000"] {
        let (status, said) = get(
            &scene,
            &scene.chief,
            &format!("{number}/transcript?last={last}"),
        )
        .await;
        assert_eq!(status, 200, "{last}");
        assert_eq!(said["total"], 400, "the copy holds them all");
        let ids: Vec<&str> = said["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids.len(), 50, "last={last}");
        assert_eq!(
            (ids[0], ids[49]),
            ("i351", "i400"),
            "the newest fifty: the 350 before them are in no answer"
        );
    }
}
