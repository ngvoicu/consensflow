//! Claude's role, loaded as Node's role-skills suite held it.

use std::fs;

use super::*;

#[test]
fn every_role_enters_every_harness_with_its_whole_text_already_loaded() {
    for name in ["chief", "advisor", "worker", "reviewer", "designer"] {
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
            message: None,
            agent: None,
            instructions: &content,
            first_message_ms: crate::testing::FIRST_MESSAGE_MS,
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
