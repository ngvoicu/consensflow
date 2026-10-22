use super::*;

/// `lsof -F ftdDin` of a running ConsensFlow 3.0.5 on this Mac, cut down: its streams, a unix
/// socket, a file a framework opened, a kqueue and a socket of the system's.
const SHOWN: &str = "p2232\nf0\ntCHR\nD0x3000002\ni336\nn/dev/null\nf2\ntREG\nD0x100000e\ni459196098\nn/Users/x/.consensflow/app/app.log\nf3\ntunix\nd0x4728cfc0aca56d7a\nn->0x1e1b0426067da685\nf6\ntREG\nD0x100000e\ni1152921500312139027\nn/System/Library/Extensions/AppleMetalOpenGLRenderer.bundle/Contents/Resources/default.metallib\nf11\ntKQUEUE\nncount=0, state=0xa\nf17\ntsystm\nd0xdcbcb4e125cc7e2f\nn[ctl com.apple.netsrc id 7 unit 58]\nfcwd\ntDIR\nD0x100000e\ni2\nn/\n";

fn held(fd: u32, kind: &str, object: Option<&str>, file: Option<(&str, u64)>, name: &str) -> Held {
    Held {
        fd,
        kind: kind.to_owned(),
        object: object.map(str::to_owned),
        file: file.map(|(device, node)| (device.to_owned(), node)),
        name: name.to_owned(),
    }
}

#[test]
fn what_lsof_says_is_read_as_each_descriptor_above_the_streams_by_its_object_or_its_file() {
    assert_eq!(
        parse_held(SHOWN),
        [
            held(3, "unix", Some("0x4728cfc0aca56d7a"), None, "->0x1e1b0426067da685"),
            held(
                6,
                "REG",
                None,
                Some(("0x100000e", 1_152_921_500_312_139_027)),
                "/System/Library/Extensions/AppleMetalOpenGLRenderer.bundle/Contents/Resources/default.metallib"
            ),
            held(11, "KQUEUE", None, None, "count=0, state=0xa"),
            held(17, "systm", Some("0xdcbcb4e125cc7e2f"), None, "[ctl com.apple.netsrc id 7 unit 58]"),
        ]
    );
}

/// The installed app as the case leaves it: a socket, a file a framework opened and a socket of
/// the system's, at numbers well above any this process has open, and what it was left, at the
/// numbers this process had them at.
fn old_with(left: &Left) -> Vec<Held> {
    vec![
        held(
            900,
            "unix",
            Some("0x4728cfc0aca56d7a"),
            None,
            "->0x1e1b0426067da685",
        ),
        held(
            901,
            "REG",
            None,
            Some(("0x100000e", 77)),
            "default.metallib",
        ),
        held(
            902,
            "systm",
            Some("0xdcbcb4e125cc7e2f"),
            None,
            "[ctl com.apple.netsrc]",
        ),
        held(left.pipe, "PIPE", Some("0xaaaa"), None, "->0xbbbb"),
        held(
            left.file_fd,
            "REG",
            None,
            Some(("0x100000e", left.node)),
            "/x/left-to-the-app",
        ),
    ]
}

#[test]
fn a_new_app_that_holds_none_of_the_old_ones_objects_has_no_fault_and_a_file_at_its_number_is_noted(
) {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    // The new app opened its own socket, and the framework's file again at the same number.
    let new = vec![
        held(900, "unix", Some("0x1111"), None, "->0x2222"),
        held(
            901,
            "REG",
            None,
            Some(("0x100000e", 77)),
            "default.metallib",
        ),
    ];
    let (faults, notes) = compare(&old, &new, &left);
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(notes.len(), 1, "{notes:?}");
}

#[test]
fn a_new_app_left_the_old_ones_descriptors_is_at_fault_for_each() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let old = old_with(&left);
    // Tauri's bare start, with nothing made close-on-exec: the new app holds them at their numbers.
    let new = old.clone();
    let (faults, _) = compare(&old, &new, &left);
    let said = faults.join("\n");
    assert!(said.contains("0xdcbcb4e125cc7e2f"), "{said}");
    assert!(said.contains("0x4728cfc0aca56d7a"), "{said}");
    assert!(said.contains("0xaaaa"), "{said}");
    assert!(
        said.contains("the file the installed app was left"),
        "{said}"
    );
}

#[test]
fn an_installed_app_that_was_not_left_the_two_makes_the_check_prove_nothing() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let (faults, _) = compare(&parse_held(SHOWN), &[], &left);
    assert_eq!(faults.len(), 2, "{faults:?}");
    assert!(faults
        .iter()
        .all(|fault| fault.ends_with("the check proves nothing")));
}

/// Not left to any program (the tests beside this one start programs): only this process
/// holds the writing end, and then none.
#[test]
fn the_pipe_left_reaches_its_end_once_no_process_holds_it_and_not_before() {
    let folder = tempfile::tempdir().unwrap();
    let mut left = Left::make(folder.path()).unwrap();
    let waits = Waits::new(std::time::Duration::from_millis(300));
    let mut held = Left::make(folder.path()).unwrap();
    assert!(held.closed_everywhere(&waits).is_err());
    left.given();
    left.closed_everywhere(&waits).unwrap();
}

#[test]
fn what_is_left_is_close_on_exec_until_it_is_left_to_the_app() {
    let folder = tempfile::tempdir().unwrap();
    let left = Left::make(folder.path()).unwrap();
    let flags = |fd: u32| fcntl(i32::try_from(fd).unwrap(), FcntlArg::F_GETFD).unwrap();
    assert_ne!(flags(left.pipe) & FdFlag::FD_CLOEXEC.bits(), 0);
    assert_ne!(flags(left.file_fd) & FdFlag::FD_CLOEXEC.bits(), 0);
}
