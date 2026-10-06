//! Which `cf` answers the standalone verbs, as a process: by default Rust
//! answers them, with no runtime named and no environment variable asked; the
//! ones it still hands to Node's sources need the Node the bundle carries (the
//! way back, which sends the rest there too, is `way_back.rs`'s); and a
//! window's token makes `cf` the board. What the verbs say is held to Node's
//! recording in `cli_goldens.rs`.

mod common;

use common::cf;

fn said(ran: &std::process::Output) -> (Option<i32>, String, String) {
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

#[test]
fn rust_answers_by_default_whatever_the_old_switch_says_and_no_runtime_is_named_for_it() {
    for args in [&["help"][..], &["--version"], &["catalog"], &["bogus"]] {
        let (_, answer, expected_err) = said(&cf(args, &[], ""));
        for stray in ["native", "node", ""] {
            // `CONSENSFLOW_DAEMON` was the switch before the flip: nothing reads it now.
            let ran = said(&cf(args, &[("CONSENSFLOW_DAEMON", stray)], ""));
            assert_eq!(
                ran,
                (ran.0, answer.clone(), expected_err.clone()),
                "{args:?}"
            );
        }
        assert!(
            !expected_err.contains("Node") && !expected_err.contains("CONSENSFLOW_NODE"),
            "{args:?}: {expected_err}"
        );
    }
    let (code, out, err) = said(&cf(&["bogus"], &[], ""));
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (
            Some(1),
            "",
            "cf: unknown command \"bogus\" — run `cf help`\n"
        )
    );
}

#[test]
fn setup_and_doctor_wait_for_the_launcher_and_go_to_the_node_of_a_bundle() {
    // Run from the build's own folder, which is no bundle: no Node is beside it.
    for verb in ["setup", "doctor"] {
        let (code, out, err) = said(&cf(&[verb], &[], ""));
        assert_eq!((code, out.as_str()), (Some(1), ""), "{verb}");
        assert!(
            err.starts_with("cf: this command runs on ConsensFlow's own Node, and none is bundled beside this cf (looked for "),
            "{verb}: {err}"
        );
    }
}

#[test]
fn a_window_token_is_the_board() {
    let (code, out, err) = said(&cf(&["help"], &[("CONSENSFLOW_TOKEN", "participant")], ""));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(out.starts_with("cf inside a ConsensFlow window"), "{out}");
}

#[test]
fn a_json_word_is_the_verbs_own_and_is_not_taken_out_before_the_verb_reads_it() {
    // The board takes `--json` out wherever it stands; the CLI's verbs are
    // handed the words as they came, so here it is the command.
    let (code, _, err) = said(&cf(&["--json", "catalog"], &[], ""));
    assert_eq!(code, Some(1));
    assert_eq!(err, "cf: unknown command \"--json\" — run `cf help`\n");
}
