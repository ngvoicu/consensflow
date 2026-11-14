use std::path::Path;

use super::*;

#[test]
fn the_item_of_a_folder_is_named_as_claude_code_names_it_for_the_path_as_it_is_given() {
    // A folder a real Claude Code 2.1.316 and 2.1.317 wrote secure storage for in the probe of
    // T-1102 (`claude mcp list` on `CLAUDE_CONFIG_DIR=/tmp/cf-keychain-vector-1`), and the item
    // the Keychain gained then, read by name from `security dump-keychain`. The path is hashed
    // as it is given: `/tmp` is a link to `/private/tmp`, and its real path names another item.
    assert_eq!(
        item_of(Path::new("/tmp/cf-keychain-vector-1")),
        "Claude Code-credentials-b0d4aa31"
    );
    assert_ne!(
        item_of(Path::new("/private/tmp/cf-keychain-vector-1")),
        "Claude Code-credentials-b0d4aa31"
    );
    // The link and its target are two folders to it, as they are to Claude Code.
    assert_ne!(
        item_of(Path::new("/tmp")),
        item_of(Path::new("/private/tmp"))
    );
}

#[test]
fn the_item_of_a_folder_is_the_services_name_and_eight_hex_digits_and_differs_by_folder() {
    let one = item_of(Path::new("/tmp/consensflow-live-claude-1-aaaa"));
    let other = item_of(Path::new("/tmp/consensflow-live-claude-1-aaab"));
    assert_ne!(one, other);
    for item in [&one, &other] {
        let hash = item.strip_prefix("Claude Code-credentials-").unwrap();
        assert_eq!(hash.len(), 8, "{item}");
        assert!(
            hash.chars()
                .all(|digit| digit.is_ascii_hexdigit() && !digit.is_ascii_uppercase()),
            "{item}"
        );
    }
    // Never the owner's own item, which has no hash: deleting what this names cannot touch it.
    assert_ne!(one, "Claude Code-credentials");
}

#[test]
fn what_is_said_of_an_item_gained_names_it_and_its_cause_and_the_command_that_deletes_it() {
    let said = gained_words("Claude Code-credentials-f2cee134");
    assert!(
        said.contains("`Claude Code-credentials-f2cee134`"),
        "{said}"
    );
    assert!(said.contains("MCP server"), "{said}");
    assert!(said.contains("deletes from the folder"), "{said}");
    assert!(
        said.contains("security delete-generic-password -s \"Claude Code-credentials-f2cee134\""),
        "{said}"
    );
    // The Keychain is the machine's: the words do not claim the run made it.
    assert!(said.contains("whole machine's"), "{said}");
    // An item without a folder's hash is not a folder's: no word of folders.
    let plain = gained_words("Claude Code-credentials");
    assert!(!plain.contains("config folder"), "{plain}");
}

#[cfg(unix)]
#[test]
fn the_security_that_finds_no_item_and_writes_none_answers_as_claude_code_reads_it() {
    use std::os::unix::fs::PermissionsExt;

    let into = crate::door::made("cf-test-").unwrap();
    let folder = without_keychain(into.path()).unwrap().unwrap();
    let security = folder.join("security");
    assert!(security.is_file());
    assert_ne!(security.metadata().unwrap().permissions().mode() & 0o111, 0);
    let run = |args: &[&str]| Run::new(&security).args(args).run().unwrap().code;
    // "No such item", for a read and for a delete: the code Claude Code reads as absent.
    assert_eq!(
        run(&["find-generic-password", "-a", "me", "-w", "-s", "x"]),
        Some(44)
    );
    assert_eq!(
        run(&["delete-generic-password", "-a", "me", "-s", "x"]),
        Some(44)
    );
    // A write fails (it is asked as `security -i` with the item on stdin, or as
    // `add-generic-password`), whatever it is told: nothing reaches the Keychain.
    assert_eq!(run(&["-i"]), Some(1));
    assert_eq!(
        run(&[
            "add-generic-password",
            "-U",
            "-a",
            "me",
            "-s",
            "x",
            "-X",
            "00"
        ]),
        Some(1)
    );
    assert_eq!(run(&["show-keychain-info"]), Some(1));
    // It writes nothing of its own beside itself.
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
}

#[cfg(not(unix))]
#[test]
fn a_machine_with_no_keychain_has_no_security_to_put_first() {
    let into = crate::door::made("cf-test-").unwrap();
    assert_eq!(without_keychain(into.path()).unwrap(), None);
}
