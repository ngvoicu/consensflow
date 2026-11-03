//! Claude's role, loaded as Node's role-skills suite held it.

use std::fs;

use super::*;

#[test]
fn every_role_enters_every_harness_with_its_whole_text_already_loaded() {
    for name in [
        "chief", "advisor", "worker", "reviewer", "tester", "designer",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_string_lossy().into_owned();
        let env = Env::from_vars([
            ("HOME", home.clone()),
            ("CONSENSFLOW_HOME", path::join(&[&home, "app"])),
        ]);
        let id = LaunchId::new("11111111-1111-4111-8111-111111111111").unwrap();
        let content =
            format!("The {name}'s whole text: \"quotes\", `backticks`, $HOME\nand newlines.");
        let launch = Launch {
            id: &id,
            project: 1,
            handle: "zeus",
            role: name,
            directory: "/work/app",
            resume: None,
            previous_gone: false,
            message: None,
            agent: None,
            instructions: &content,
            specs: None,
            compaction: None,
            first_message_ms: crate::testing::FIRST_MESSAGE_MS,
            starts_given_up: 0,
        };
        let args = role(&env, &launch).unwrap();
        let at = args
            .iter()
            .position(|arg| arg == "--append-system-prompt-file")
            .expect("the full role must enter the native system prompt");
        let file = &args[at + 1];
        assert!(
            file.replace('\\', "/")
                .ends_with(&format!("/consensflow-{name}/SKILL.md")),
            "{file}"
        );
        assert_eq!(
            fs::read_to_string(file).unwrap(),
            content,
            "the whole role text is loaded"
        );
        let snapshot = args
            .iter()
            .position(|arg| arg == "--system-prompt-snapshot")
            .unwrap();
        assert_eq!(
            args[snapshot + 1],
            "off",
            "resumed conversations must use the current role instructions"
        );
    }
}

#[cfg(not(windows))]
#[test]
fn only_the_libraries_a_folder_is_not_in_are_denied_on_a_mac() {
    // The reads a Mac's window is denied (T-1194).
    let mac = Env::from_vars([("HOME", "/Users/dmitry")]);
    assert_eq!(
        private_reads(&mac, "/work/app"),
        [
            "Read(~/Music)",
            "Read(~/Music/**)",
            "Read(~/Pictures/*.photoslibrary)",
            "Read(~/Pictures/*.photoslibrary/**)"
        ]
    );
    // A project in Music is not kept out of its own folder, nor one in a Photos library.
    assert_eq!(private_reads(&mac, "/Users/dmitry/Music").len(), 2);
    assert_eq!(private_reads(&mac, "/Users/dmitry/Music/songs/").len(), 2);
    assert_eq!(
        private_reads(
            &mac,
            "/Users/dmitry/Pictures/Photos Library.photoslibrary/originals"
        ),
        ["Read(~/Music)", "Read(~/Music/**)"]
    );
    // Music's neighbours are ordinary folders.
    assert_eq!(private_reads(&mac, "/Users/dmitry/Musical").len(), 4);
    assert_eq!(private_reads(&mac, "/Users/dmitry").len(), 4);
}

#[test]
fn nothing_is_denied_on_windows_wherever_the_folder_is() {
    // Windows asks nothing of the kind: its file is as it always was. An environment that says
    // it is Windows's is, on a Mac as on Windows, and a Windows build is whatever it is given.
    let windows = Env::from_vars([("HOME", "C:\\Users\\dmitry"), ("OS", "Windows_NT")]);
    assert!(private_reads(&windows, "C:\\work\\app").is_empty());
    assert!(private_reads(&windows, "C:\\Users\\dmitry\\Music").is_empty());
}
