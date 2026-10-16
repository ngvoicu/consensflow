//! How far back `cf task get T-n --transcript` reaches (the field report of
//! 2026-10-15, B7, where `--last 400` did not reach the window that overwrote a
//! file). The copy keeps every item (`copy_transcript`); `?last=` gives the last
//! 50 at most, as it always did, and `?page=` and `?find=` reach the rest, 50 to
//! a page from the newest, searched in any case, as `cf history` is read.

use serde_json::{json, Value};

use super::{get, refused, written_by_zeus};
use crate::api::routes::tests::support::working_task;
use crate::testing::{scene, Scene};

/// The ids of the items an answer gives.
fn ids(said: &Value) -> Vec<&str> {
    said["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect()
}

/// A task whose window wrote 400 items, `item 1` to `item 400`, and its number.
fn four_hundred(scene: &Scene) -> i64 {
    let number = working_task(scene);
    written_by_zeus(scene, 400);
    number
}

#[tokio::test]
async fn the_last_items_are_the_newest_fifty_at_most_as_they_always_were() {
    let scene = scene();
    let number = four_hundred(&scene);
    for last in ["400", "50", "1000"] {
        let route = format!("{number}/transcript?last={last}");
        let (status, said) = get(&scene, &scene.chief, &route).await;
        assert_eq!(status, 200, "{last}");
        assert_eq!(said["total"], 400, "the copy holds them all");
        assert_eq!(said.get("page"), None, "no page: the last items");
        let ids = ids(&said);
        assert_eq!(ids.len(), 50, "last={last}");
        assert_eq!((ids[0], ids[49]), ("i351", "i400"));
    }
}

#[tokio::test]
async fn a_page_reaches_back_fifty_items_at_a_time_from_the_newest() {
    let scene = scene();
    let number = four_hundred(&scene);
    for (page, first, last) in [
        ("1", "i351", "i400"),
        ("2", "i301", "i350"),
        ("8", "i1", "i50"),
    ] {
        let route = format!("{number}/transcript?page={page}");
        let (status, said) = get(&scene, &scene.chief, &route).await;
        assert_eq!(status, 200, "page {page}");
        assert_eq!(
            [&said["page"], &said["pages"], &said["total"]],
            [&json!(page.parse::<u64>().unwrap()), &json!(8), &json!(400)]
        );
        assert_eq!(said.get("found"), None, "no search");
        let ids = ids(&said);
        assert_eq!(
            (ids.len(), ids[0], ids[49]),
            (50, first, last),
            "page {page}"
        );
    }
    // A page that is none of them is refused in words that say how many there are.
    for page in ["9", "0", "1.5", "abc"] {
        let route = format!("{number}/transcript?page={page}");
        let said = get(&scene, &scene.chief, &route).await;
        assert_eq!(
            refused(&said),
            (400, "no-such-page", "there are 8 pages"),
            "page {page}"
        );
    }
}

#[tokio::test]
async fn a_search_keeps_the_items_that_say_its_words_in_any_case_and_pages_them() {
    let scene = scene();
    let number = four_hundred(&scene);
    let route = format!("{number}/transcript?find=ITEM%2039");
    let (status, said) = get(&scene, &scene.chief, &route).await;
    assert_eq!(status, 200);
    assert_eq!(
        [
            &said["found"],
            &said["total"],
            &said["page"],
            &said["pages"]
        ],
        [&json!(11), &json!(400), &json!(1), &json!(1)]
    );
    assert_eq!(
        ids(&said),
        ["i39", "i390", "i391", "i392", "i393", "i394", "i395", "i396", "i397", "i398", "i399"]
    );

    // Words nothing says: no page to give, and nothing refused.
    let route = format!("{number}/transcript?find=overwrote&page=3");
    let (status, said) = get(&scene, &scene.chief, &route).await;
    assert_eq!(status, 200);
    assert_eq!(
        [
            &said["found"],
            &said["page"],
            &said["pages"],
            &said["items"]
        ],
        [&json!(0), &json!(0), &json!(0), &json!([])]
    );

    // An empty search is none: the last items.
    let route = format!("{number}/transcript?find=");
    let (_, said) = get(&scene, &scene.chief, &route).await;
    assert_eq!((said.get("page"), ids(&said).len()), (None, 10));
}
