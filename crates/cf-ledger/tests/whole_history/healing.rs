//! A copy made by a daemon that read an item otherwise is mended by the copy that follows
//! it. Claude Code's companion record (the sources of the images it attached to a message,
//! `turnCompanion`) was copied as a `user` item until T-265 read it as the harness's own
//! (`custom`); the full copy of a conversation that a daemon makes as it starts (T-296) must
//! bring such an item to the role it now has, in the copy `cf history` reads and in the
//! heads the history viewer reads.

use super::*;

#[test]
fn an_item_copied_as_the_users_and_copied_again_as_the_harness_s_own_is_read_so_by_both_readers() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut ledger = ledger_in(&dir);
    let project = project(&mut ledger);
    let chief = id_of(&project, "chief");
    let first = ledger
        .start_conversation(chief, "claude-code")
        .expect("a conversation");
    ledger.bind_conversation(first.id, "s-1").expect("bound");
    let sources = "[Image: source: /pictures/desk.png]";

    // A daemon that took the companion for the user's own item copied it so.
    let before = [
        item("u-1", "user", "Look at this"),
        item("c-1", "user", sources),
    ];
    assert_eq!(
        ledger
            .copy_transcript(first.id, &before, 0)
            .expect("copied"),
        2
    );
    // This one reads it as the harness's own, and the copy it makes as it starts mends it:
    // the one row that changed is written, and a copy again changes none.
    let now = [
        item("u-1", "user", "Look at this"),
        item("c-1", "custom", sources),
    ];
    assert_eq!(
        ledger.copy_transcript(first.id, &now, 0).expect("copied"),
        1,
        "the item whose role changed, and no other"
    );
    assert_eq!(
        ledger.copy_transcript(first.id, &now, 0).expect("copied"),
        0,
        "and then nothing"
    );

    // The conversation ends, so `cf history` reads it.
    ledger
        .follow_conversation(chief, "claude-code", "s-2")
        .expect("followed");
    let roles = |items: &[HistoryItem]| -> Vec<String> {
        items.iter().map(|item| item.role.clone()).collect()
    };
    let viewer = everything(&ledger, project.id, "chief").expect("the history");
    assert_eq!(
        roles(&viewer[0].1),
        ["user", "custom"],
        "the viewer's heads"
    );
    let earlier = ledger.chief_history(project.id).expect("the history");
    assert_eq!(roles(&earlier[0].items), ["user", "custom"], "`cf history`");
}
