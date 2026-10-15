//! A brief read from standard input (`cf task add --tier standard - <<'BRIEF'`) is
//! taken as the shell gave it: the evidence for the field report of 2026-10-15
//! (batch B, B6), where a chief's heredoc lost its closing word, bash warned
//! "here-document … delimited by end-of-file", and the brief went to the board cut at
//! about 8.1k characters. `cf` reads its input to the end and drops one final line
//! break; nothing in what it is given says the shell cut it, and what it prints of
//! the task says nothing of its size or of how it ends. What to do about it is the
//! owner's (the brain's `issues/field-reports-2026-10-15.md`).

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use cf_board::scripted::{reply, scripted};
use common::cf;
use serde_json::json;

#[test]
fn a_brief_cut_in_the_middle_of_a_fence_is_posted_as_given_and_nothing_said_of_it() {
    // 8,100 characters, and then a fence opened and never closed: the shape of a
    // heredoc whose closing word the shell never had.
    let cut = format!(
        "{}\n\n```rust\nfn main() {{\n    let parser = Parser::new(",
        "Write the parser. ".repeat(450)
    );
    let api = scripted(vec![reply(
        201,
        json!({
            "task": {
                "number": 1, "state": "open", "assignee": null, "requester": "chief",
                "title": "Write the parser.", "blockedBy": [], "pool": "worker", "tier": "standard",
            },
            "message": null, "gated": false,
        }),
    )]);
    let ran = cf(
        &["task", "add", "--tier", "standard", "-"],
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        &cut,
    );
    assert_eq!(ran.status.code(), Some(0));
    let posted = api.received().into_iter().next().and_then(|got| got.json());
    assert_eq!(
        posted,
        Some(json!({ "tier": "standard", "body": cut })),
        "the brief, as cut"
    );
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "T-1 is on the board for a standard worker; the first free one gets it, and its result arrives in your inbox.\n",
        "no size, and no word of how it ends"
    );
    assert!(ran.stderr.is_empty());
}
