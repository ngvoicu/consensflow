//! The way back to Node as a process: a home that has the `use-node` file in
//! it sends every tokenless `cf` command to the CLI's Node sources, on the Node
//! the bundle carries, which `cf` finds from its own place in the bundle (no
//! `CONSENSFLOW_NODE`: a terminal has none). A window's token makes `cf` the
//! board whatever the file says. Without the file nothing runs on Node: every
//! verb is this binary's own, `setup` and `doctor` too (`standalone.rs` runs
//! those two, in a user's home of their own, which this file's runs have not).
//!
//! The bundle is laid out as the app's is (`common::Bundle`), with a stand-in
//! Node that says it ran, with what it was given: a shell script, which Windows
//! has no Node of. There only where Node is looked for is held.

mod common;

use common::{Bundle, Home};

/// What the stand-in Node does: says it ran, and with what, and exits 3.
#[cfg(unix)]
const STAND_IN: &str = "printf 'ran|%s|' \"$0\"; printf '%s|' \"$@\"; exit 3";

fn said(ran: &std::process::Output) -> (Option<i32>, String, String) {
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

/// What the stand-in Node says for `args`: its own path, the sources it was
/// given first, and the words after them.
#[cfg(unix)]
fn ran_on_node(bundle: &Bundle, args: &[&str]) -> String {
    format!(
        "ran|{}|{}|{}|",
        bundle.node.display(),
        bundle.cf_mjs.display(),
        args.join("|")
    )
}

#[cfg(unix)]
#[test]
fn with_the_file_every_tokenless_verb_runs_on_the_node_beside_cf_with_none_named() {
    let bundle = Bundle::new(Some(STAND_IN));
    let home = Home::new(true);
    for args in [
        &["help"][..],
        &["--version"],
        &["catalog", "--harness", "two words"],
        &["agent", "list", "--json"],
        &["setup"],
        &["doctor"],
        &["ui", "--json", "--no-open"],
        &["frobnicate"],
    ] {
        let (code, out, err) = said(&bundle.cf(args, &home, &[]));
        assert_eq!(
            (code, out, err.as_str()),
            (Some(3), ran_on_node(&bundle, args), ""),
            "{args:?}"
        );
    }
    // The daemon was not started by `cf ui`: the Node that was started is the one.
    assert!(!home.path().join("daemon.log").exists());
}

#[cfg(unix)]
#[test]
fn the_node_an_environment_names_is_not_the_one_that_runs() {
    // A window has `CONSENSFLOW_NODE`, a terminal has none: a `cf` that asked for
    // it would start what is named here, which is nothing.
    let bundle = Bundle::new(Some(STAND_IN));
    let home = Home::new(true);
    let ran = bundle.cf(
        &["catalog"],
        &home,
        &[("CONSENSFLOW_NODE", "/nonexistent/node")],
    );
    assert_eq!(said(&ran).1, ran_on_node(&bundle, &["catalog"]));
}

#[cfg(unix)]
#[test]
fn the_old_switch_in_the_environment_changes_nothing_either_way() {
    // `CONSENSFLOW_DAEMON` was the switch before the flip. A terminal does not
    // inherit the app's environment, so a variable could not be the way back: the
    // home's file is, and nothing else decides.
    let bundle = Bundle::new(Some(STAND_IN));
    for stray in ["native", "node", ""] {
        let back = Home::new(true);
        let (_, out, _) = said(&bundle.cf(&["catalog"], &back, &[("CONSENSFLOW_DAEMON", stray)]));
        assert_eq!(
            out,
            ran_on_node(&bundle, &["catalog"]),
            "{stray:?}, the file"
        );
        let plain = Home::new(false);
        let (_, out, _) = said(&bundle.cf(&["catalog"], &plain, &[("CONSENSFLOW_DAEMON", stray)]));
        assert!(!out.starts_with("ran|"), "{stray:?}, no file: {out}");
    }
}

#[cfg(unix)]
#[test]
fn without_the_file_cf_answers_every_verb_by_itself() {
    let bundle = Bundle::new(Some(STAND_IN));
    let home = Home::new(false);
    for args in [
        &["help"][..],
        &["--version"],
        &["catalog"],
        &["agent", "list"],
        &["frobnicate"],
    ] {
        let (code, out, _) = said(&bundle.cf(args, &home, &[]));
        assert!(!out.starts_with("ran|"), "{args:?} ran on Node: {out}");
        assert_ne!(code, Some(3), "{args:?} ran on Node");
    }
}

#[cfg(unix)]
#[test]
fn a_window_token_is_the_board_whatever_the_file_says() {
    let bundle = Bundle::new(Some(STAND_IN));
    let home = Home::new(true);
    let (code, out, err) = said(&bundle.cf(&["help"], &home, &[("CONSENSFLOW_TOKEN", "tok")]));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert!(out.starts_with("cf inside a ConsensFlow window"), "{out}");
    // A window's `cf ui` is a board command, which the board refuses.
    let (code, out, _) = said(&bundle.cf(
        &["ui", "--json", "--no-open"],
        &home,
        &[
            ("CONSENSFLOW_TOKEN", "tok"),
            ("CONSENSFLOW_URL", "http://127.0.0.1:9"),
        ],
    ));
    assert_ne!(code, Some(3));
    assert!(!out.starts_with("ran|"), "{out}");
}

#[test]
fn a_bundle_with_no_node_says_so_and_how_to_be_rid_of_the_file() {
    let bundle = Bundle::new(None);
    let home = Home::new(true);
    let (code, out, err) = said(&bundle.cf(&["catalog"], &home, &[]));
    assert_eq!((code, out.as_str()), (Some(1), ""));
    let file = home.path().join("use-node");
    assert!(
        err.starts_with(&format!(
            "cf: {} sends this home's commands to Node, and none is bundled beside this cf (looked for ",
            file.display()
        )),
        "{err}"
    );
    assert!(
        err.ends_with(&format!(
            "): delete the file to run the native cf, or run {} with a node of your choosing.\n",
            bundle.cf_mjs.display()
        )),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn a_node_that_does_not_start_is_said_and_fails() {
    use std::os::unix::fs::PermissionsExt;
    let bundle = Bundle::new(Some(STAND_IN));
    std::fs::set_permissions(&bundle.node, std::fs::Permissions::from_mode(0o644))
        .expect("a node that does not run");
    let home = Home::new(true);
    let (code, out, err) = said(&bundle.cf(&["catalog"], &home, &[]));
    assert_eq!((code, out.as_str()), (Some(1), ""));
    assert!(
        err.starts_with(&format!("cf: {} did not start:", bundle.node.display())),
        "{err}"
    );
}
