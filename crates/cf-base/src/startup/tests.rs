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
fn every_kind_has_a_status_of_its_own_in_a_range_no_harness_uses_and_reads_back() {
    let mut seen = Vec::new();
    for kind in StartKind::ALL {
        let status = kind.exit_status();
        assert!((70..78).contains(&status), "{kind:?}: {status}");
        assert_eq!(StartKind::of_exit_status(i64::from(status)), Some(kind));
        assert!(!seen.contains(&status), "{kind:?}: {status} is another's");
        seen.push(status);
    }
    for other in [-1, 0, 1, 2, 64, 69, 78, 126, 127, 130, 255, 0xC000_013A] {
        assert_eq!(StartKind::of_exit_status(other), None, "{other}");
    }
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
