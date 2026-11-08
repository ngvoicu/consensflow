use super::*;

#[test]
fn a_failure_is_said_by_its_kind_and_its_code_and_its_advice_and_nothing_else() {
    let not_found = StartFailure::of(StartKind::NotFound).with_errno("ENOENT");
    assert_eq!(not_found.what("Pi"), "Pi's program was not found (ENOENT)");
    assert_eq!(
        not_found.words("Pi"),
        "Pi's program was not found (ENOENT); check that Pi is installed and that ConsensFlow can find it"
    );
    let no_node = StartFailure::of(StartKind::NoNodeForPi);
    assert_eq!(
        no_node.words("Pi"),
        "Node.js was not found for Pi; install Node 22.19 or newer, or run Pi's installer again"
    );
    assert_eq!(
        StartFailure::of(StartKind::NoNode).words("Codex"),
        "Node.js was not found for Codex; make the harness's Node visible to ConsensFlow, or install the harness's own build instead of the npm one"
    );
    assert_eq!(
        StartFailure::of(StartKind::Failed).with_exit(3).words("Pi"),
        "Pi's program ended with an error (exit code 3)"
    );
    assert_eq!(
        StartFailure::of(StartKind::NotAnNpmShim).words("Codex"),
        "Codex's program is a script that is not an npm shim; install Codex's own build, or its npm shim"
    );
}

#[test]
fn every_kind_has_a_status_of_its_own_in_a_range_of_consensflows_and_reads_back() {
    let (first, last) = StartKind::exit_statuses();
    let mut seen = Vec::new();
    for kind in StartKind::ALL {
        let status = kind.exit_status();
        assert!((first..=last).contains(&status), "{kind:?}: {status}");
        assert_eq!(StartKind::of_exit_status(i64::from(status)), Some(kind));
        assert!(!seen.contains(&status), "{kind:?}: {status} is another's");
        seen.push(status);
    }
    assert_eq!(
        seen.len(),
        usize::try_from(last - first + 1).unwrap(),
        "no status of the range is unused"
    );
    for other in [-1, 0, 1, 2, 64, 69, 81, 126, 127, 130, 255, 0xC000_013A] {
        assert_eq!(StartKind::of_exit_status(other), None, "{other}");
    }
}

/// T-1328, the third reading: a status of the range is never a harness's, whatever the harness
/// ended with. Every status a Unix program can end with, and the Windows ones around the range and
/// above it: those of the range are 1, every other is itself, and none of what is reported is in
/// the range but what ConsensFlow's own program ends with.
#[test]
fn a_status_a_harness_ended_with_is_reported_as_itself_but_for_those_of_the_range_which_are_1() {
    let (first, last) = StartKind::exit_statuses();
    let windows = [256, 326, 0x1_0000, i32::MAX, -1, -1_073_741_819, i32::MIN];
    for code in (0..=255).chain(windows) {
        let reported = harness_exit(code);
        if (first..=last).contains(&code) {
            assert_eq!(reported, 1, "{code}");
        } else {
            assert_eq!(reported, code, "{code}");
        }
        assert_eq!(
            StartKind::of_exit_status(i64::from(reported)),
            None,
            "{code} is reported as {reported}, a status of why"
        );
    }
    for kind in StartKind::ALL {
        assert_eq!(
            StartKind::of_exit_status(i64::from(kind.exit_status())),
            Some(kind)
        );
    }
}

#[test]
fn a_session_that_did_not_get_the_harness_ready_says_so_in_consensflows_words_and_the_exit_as_windows_writes_it(
) {
    assert_eq!(
        StartFailure::of(StartKind::ServerStopped).with_exit(127).words("Codex"),
        "Codex's server stopped before it was ready (exit code 127); its cause is in ConsensFlow's log"
    );
    assert_eq!(
        StartFailure::of(StartKind::ServerStopped)
            .with_exit(-1_073_741_819)
            .what("Codex"),
        "Codex's server stopped before it was ready (exit code 0xC0000005)"
    );
    assert_eq!(
        StartFailure::of(StartKind::ServerStopped).words("Codex"),
        "Codex's server stopped before it was ready; its cause is in ConsensFlow's log"
    );
    assert_eq!(
        StartFailure::of(StartKind::ServerSilent).words("Codex"),
        "Codex's server did not come up in time; its cause is in ConsensFlow's log"
    );
    assert_eq!(
        StartFailure::of(StartKind::NotSetUp)
            .with_errno("ENOENT")
            .words("Codex"),
        "ConsensFlow could not set up Codex's window (ENOENT); its cause is in ConsensFlow's log"
    );
}

#[test]
fn a_refusal_keeps_the_persons_words_apart_from_what_a_note_says() {
    let refusal = StartRefusal::new(
        StartFailure::of(StartKind::NoNode),
        r"C:\tools\pi hunter22.cmd runs on Node".to_owned(),
    );
    assert_eq!(
        refusal.to_string(),
        r"C:\tools\pi hunter22.cmd runs on Node"
    );
    assert_eq!(String::from(refusal.clone()), refusal.words());
    assert!(!refusal.failure().words("Pi").contains("hunter22"));
}

#[test]
fn the_systems_name_for_a_failed_start_says_a_kind_and_a_code_but_for_unknown() {
    assert_eq!(
        StartFailure::of_errno("ENOENT"),
        StartFailure::of(StartKind::NotFound).with_errno("ENOENT")
    );
    for name in ["EACCES", "EPERM"] {
        assert_eq!(
            StartFailure::of_errno(name),
            StartFailure::of(StartKind::NotAllowed).with_errno(name)
        );
    }
    assert_eq!(
        StartFailure::of_errno("ENOEXEC"),
        StartFailure::of(StartKind::NotStarted).with_errno("ENOEXEC")
    );
    // libuv's UNKNOWN names nothing.
    assert_eq!(
        StartFailure::of_errno("UNKNOWN"),
        StartFailure::of(StartKind::NotStarted)
    );
}
