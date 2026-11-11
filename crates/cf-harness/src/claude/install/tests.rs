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
            advisor_off: false,
            suggestion_off: false,
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
            format!("---\ndisable-model-invocation: true\n---\n\n{content}"),
            "the whole role text is loaded, and no model may load it as a skill"
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

#[test]
fn a_roles_front_matter_says_no_model_may_load_it_as_a_skill_and_keeps_the_rest_as_it_was() {
    let role = "---\nname: consensflow-worker\ndescription: Carry out tasks.\n---\n\n# ConsensFlow worker\n\nYou carry out tasks.\n";
    assert_eq!(
        not_for_the_model(role),
        "---\ndisable-model-invocation: true\nname: consensflow-worker\ndescription: Carry out tasks.\n---\n\n# ConsensFlow worker\n\nYou carry out tasks.\n"
    );
    // A text with no front matter of its own (an evaluation's card) is given one.
    assert_eq!(
        not_for_the_model("# A card\n"),
        "---\ndisable-model-invocation: true\n---\n\n# A card\n"
    );
    // A rule of three dashes further down is no front matter.
    assert_eq!(
        not_for_the_model("# A card\n---\nmore\n"),
        "---\ndisable-model-invocation: true\n---\n\n# A card\n---\nmore\n"
    );
    assert_eq!(
        not_for_the_model(""),
        "",
        "an empty role is refused, not filled"
    );
}

#[test]
fn the_roles_skill_file_on_disk_is_the_one_whose_front_matter_hides_it_from_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_string_lossy().into_owned();
    let env = Env::from_vars([
        ("HOME", home.clone()),
        ("CONSENSFLOW_HOME", path::join(&[&home, "app"])),
    ]);
    let id = LaunchId::new("11111111-1111-4111-8111-111111111111").unwrap();
    let content = "---\nname: consensflow-chief\ndescription: Run the project.\n---\n\n# Chief\n";
    let launch = Launch {
        id: &id,
        project: 1,
        handle: "chief",
        role: "chief",
        directory: "/work/app",
        resume: None,
        previous_gone: false,
        message: None,
        agent: None,
        instructions: content,
        specs: None,
        first_message_ms: crate::testing::FIRST_MESSAGE_MS,
        starts_given_up: 0,
        compaction: None,
        advisor_off: false,
        suggestion_off: false,
    };
    let args = role(&env, &launch).unwrap();
    let folder = &args[args.iter().position(|arg| arg == "--add-dir").unwrap() + 1];
    let skill = std::path::Path::new(folder)
        .join(".claude")
        .join("skills")
        .join("consensflow-chief")
        .join("SKILL.md");
    let text = fs::read_to_string(&skill).unwrap();
    let front: Vec<&str> = text.lines().take_while(|line| *line != "# Chief").collect();
    assert_eq!(
        front,
        [
            "---",
            "disable-model-invocation: true",
            "name: consensflow-chief",
            "description: Run the project.",
            "---",
            ""
        ]
    );
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
