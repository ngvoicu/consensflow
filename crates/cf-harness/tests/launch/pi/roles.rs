//! The role a Pi window is given, as the Pi cases of Node's role-skills suite
//! held them: its whole text, in a file of the launch's own and in the system
//! prompt.

use super::*;

#[test]
fn every_role_enters_pi_with_its_whole_text_already_loaded() {
    for name in ["chief", "advisor", "worker", "reviewer", "designer"] {
        let home = Home::new();
        let (adapter, _fakes) = adapter(&home);
        let content = format!(
            "The {name}'s whole text: \"quotes\", `backticks`, $HOME\nand newlines.\n| saved-worker |"
        );
        let request = Request {
            instructions: content.clone(),
            role: name,
            ..Request::default()
        };
        let plan = prepare(&adapter, &request).unwrap();
        // `--skill` alone only advertises the role: its text is appended to the prompt.
        assert_eq!(plan.argv[3], "--skill");
        let file = &plan.argv[4];
        assert!(
            file.replace('\\', "/")
                .ends_with(&format!("/consensflow-{name}/SKILL.md")),
            "{file}"
        );
        assert_eq!(fs::read_to_string(file).unwrap(), content);
        let at = plan
            .argv
            .iter()
            .position(|arg| arg == "--append-system-prompt")
            .expect("--skill alone only advertises the role");
        assert_eq!(plan.argv[at + 1], content, "the whole role text is loaded");
    }
}

#[test]
fn a_role_text_is_private_to_its_launch_and_is_given_to_pi_whole() {
    let home = Home::new();
    let (adapter, _fakes) = adapter(&home);
    let request = Request {
        instructions: "worker text".to_owned(),
        ..Request::default()
    };
    let plan = prepare(&adapter, &request).unwrap();
    let file = &plan.argv[4];
    assert_eq!(fs::read_to_string(file).unwrap(), "worker text");
    assert_eq!(plan.argv[6], "worker text");
    assert!(file.starts_with(&home.launch_folder()));
    // Windows has no POSIX modes; its files answer 0o666 whatever the writer asked.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
