//! Which `cf` answers the standalone verbs, as a process: the ones Rust answers
//! once the switch (`CONSENSFLOW_DAEMON=native`) is on, with no runtime
//! named; the ones it still hands to Node's sources; and a window's token,
//! which makes `cf` the board whatever the switch says. What the verbs say is
//! held to Node's recording in `cli_goldens.rs`.

mod common;

use common::cf;

const NODE_NEEDED: &str = "cf: CONSENSFLOW_NODE is not set:";

fn said(ran: &std::process::Output) -> (Option<i32>, String, String) {
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn the_switch_is_what_turns_the_verbs_on_and_no_runtime_is_named_for_them() {
    for args in [&["help"][..], &["--version"], &["catalog"], &["bogus"]] {
        // Off: Node's sources answer, and none is named.
        let (code, out, err) = said(&cf(args, &[], ""));
        assert_eq!((code, out.as_str()), (Some(1), ""), "{args:?}");
        assert!(err.starts_with(NODE_NEEDED), "{args:?}: {err}");
        // On: Rust answers, and still none is named.
        let (_, _, err) = said(&cf(args, &[("CONSENSFLOW_DAEMON", "native")], ""));
        assert!(!err.contains("CONSENSFLOW_NODE"), "{args:?}: {err}");
    }
}

#[test]
fn setup_and_doctor_wait_for_the_launcher_and_go_to_node_with_the_switch_on() {
    for verb in ["setup", "doctor"] {
        let (code, out, err) = said(&cf(&[verb], &[("CONSENSFLOW_DAEMON", "native")], ""));
        assert_eq!((code, out.as_str()), (Some(1), ""), "{verb}");
        assert!(err.starts_with(NODE_NEEDED), "{verb}: {err}");
    }
}

#[test]
fn a_window_token_is_the_board_whatever_the_switch_says() {
    let (code, out, err) = said(&cf(
        &["help"],
        &[
            ("CONSENSFLOW_DAEMON", "native"),
            ("CONSENSFLOW_TOKEN", "participant"),
        ],
        "",
    ));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(out.starts_with("cf inside a ConsensFlow window"), "{out}");
}

#[test]
fn a_json_word_is_the_verbs_own_and_is_not_taken_out_before_the_verb_reads_it() {
    // The board takes `--json` out wherever it stands; the CLI's verbs are
    // handed the words as they came, so here it is the command.
    let (code, _, err) = said(&cf(
        &["--json", "catalog"],
        &[("CONSENSFLOW_DAEMON", "native")],
        "",
    ));
    assert_eq!(code, Some(1));
    assert_eq!(err, "cf: unknown command \"--json\" — run `cf help`\n");
}
