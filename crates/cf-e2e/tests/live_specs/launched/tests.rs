use serde_json::json;

use super::*;

const SECTION_TEXT: &str = "\n## This project's specs\n\nSpecs are on in this project. Plan work that is big enough for phases as a spec, with the Specmint skill `specmint-tdd`: its SKILL.md is at /home/me/.consensflow/integrations/specmint/claude/tdd-0123abcd/.claude/skills/specmint-tdd/SKILL.md, and the files it names are beside it; read it before you plan. Your specs and their registry are in `plans/specs`: use this copy of the skill and no other; its paths name it.\n";

#[test]
fn a_text_with_specs_off_says_nothing_of_them_and_one_that_says_a_word_of_them_is_refused() {
    assert!(told_nothing_of_specs("You run this project.\n## This window's cf\n").is_ok());
    assert!(!has_the_section("You run this project.\n"));
    let failed = told_nothing_of_specs(SECTION_TEXT).unwrap_err();
    assert!(failed.contains("This project's specs"), "{failed}");
    assert!(has_the_section(SECTION_TEXT));
    assert!(told_nothing_of_specs("use Specmint").is_err());
}

#[test]
fn the_skill_and_the_folder_the_text_names_are_read_from_the_words_the_section_says_them_in() {
    assert_eq!(
        skill_named_in(SECTION_TEXT).as_deref(),
        Some(
            "/home/me/.consensflow/integrations/specmint/claude/tdd-0123abcd/.claude/skills/specmint-tdd/SKILL.md"
        )
    );
    assert_eq!(
        folder_named_in(SECTION_TEXT).as_deref(),
        Some("plans/specs")
    );
    assert_eq!(skill_named_in("nothing here"), None);
    assert_eq!(folder_named_in("nothing here"), None);
    // A path with spaces, as a Windows one has.
    let spaced =
        "its SKILL.md is at C:/Program Files/cf/SKILL.md, and the files it names are beside it";
    assert_eq!(
        skill_named_in(spaced).as_deref(),
        Some("C:/Program Files/cf/SKILL.md")
    );
}

#[test]
fn a_skill_is_called_what_its_frontmatter_says() {
    assert_eq!(
        skill_called("---\nname: specmint-tdd\ndescription: x\n---\nbody\nname: other\n")
            .as_deref(),
        Some("specmint-tdd")
    );
    assert_eq!(
        skill_called("---\ndescription: x\nname: \"specmint-core\"\n---\n").as_deref(),
        Some("specmint-core")
    );
    // A name below the frontmatter is the body's, and a file with none has none.
    assert_eq!(
        skill_called("---\ndescription: x\n---\nname: other\n"),
        None
    );
    assert_eq!(skill_called("name: specmint-core\n"), None);
    assert_eq!(skill_called(""), None);
}

fn frame(argv: &Value, env: &Value) -> Value {
    json!({ "id": "p1-chief", "argv": argv, "env": env })
}

#[test]
fn each_harness_is_given_its_role_text_where_its_adapter_puts_it_and_the_run_finds_it_there() {
    // Claude: a file its system prompt appends.
    assert_eq!(
        role_of(&frame(
            &json!([
                "claude",
                "--add-dir",
                "/r",
                "--append-system-prompt-file",
                "/r/f",
                "--model",
                "m"
            ]),
            &json!({})
        )),
        Some(RoleText::File("/r/f".to_owned()))
    );
    // Pi: the text itself, appended to its system prompt.
    assert_eq!(
        role_of(&frame(
            &json!(["pi", "--skill", "/s", "--append-system-prompt", "the role"]),
            &json!({})
        )),
        Some(RoleText::Inline("the role".to_owned()))
    );
    // Pi 1.4.0: the user's own `APPEND_SYSTEM.md` first and the role's file last, which Pi reads
    // as text where the path is a file (T-662: the run read the path as the text and found
    // no section of specs in it).
    assert_eq!(
        role_of(&frame(
            &json!([
                "pi",
                "--append-system-prompt",
                "/h/.pi/agent/APPEND_SYSTEM.md",
                "--append-system-prompt",
                "/r/role/SKILL.md"
            ]),
            &json!({})
        )),
        Some(RoleText::Inline("/r/role/SKILL.md".to_owned()))
    );
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("SKILL.md");
    std::fs::write(&file, "the role in a file").unwrap();
    assert_eq!(
        text_of(&file.display().to_string()).unwrap(),
        "the role in a file"
    );
    assert_eq!(text_of("the role").unwrap(), "the role");
    // Codex: a file its broker reads, named in its environment.
    assert_eq!(
        role_of(&frame(
            &json!(["cf", "codex-session", "/bin/codex"]),
            &json!({ "CF_CODEX_ROLE_FILE": "/c/role" })
        )),
        Some(RoleText::File("/c/role".to_owned()))
    );
    // Devin: a file its session-start hook reads, named in its environment.
    assert_eq!(
        role_of(&frame(
            &json!(["devin"]),
            &json!({ "CF_DEVIN_ROLE_FILE": "/d/role" })
        )),
        Some(RoleText::File("/d/role".to_owned()))
    );
    // A window given its role in none of these ways is one the run cannot read.
    assert_eq!(role_of(&frame(&json!(["claude"]), &json!({}))), None);
}

#[test]
fn a_window_of_no_specs_names_no_skill_on_its_command_line_or_in_its_environment() {
    let clean = frame(
        &json!(["claude", "--add-dir", "/r", "--model", "fake"]),
        &json!({ "CONSENSFLOW_URL": "http://127.0.0.1:1" }),
    );
    carries_no_skill(&clean).unwrap();
    let on_argv = frame(
        &json!(["pi", "--skill", "/r/skills/specmint-core/SKILL.md"]),
        &json!({}),
    );
    let failed = carries_no_skill(&on_argv).unwrap_err();
    assert!(
        failed.starts_with("the window's command line names a Specmint skill: "),
        "{failed}"
    );
    assert!(failed.contains("specmint-core"), "{failed}");
    let in_env = frame(
        &json!(["devin"]),
        &json!({ "CF_DEVIN_ROLE_FILE": "/r/skills/specmint-tdd/SKILL.md" }),
    );
    let failed = carries_no_skill(&in_env).unwrap_err();
    assert!(
        failed.starts_with("the window's environment names a Specmint skill: CF_DEVIN_ROLE_FILE="),
        "{failed}"
    );
}

#[test]
fn what_codex_says_of_the_human_s_own_skills_is_no_skill_of_the_window_and_no_text_of_ours() {
    let codex = format!(
        "Codex's own words, and the human's: a skill called specmint-core is installed.\n\nYour ConsensFlow role is chief. The following role instructions are already loaded.{SECTION_TEXT}"
    );
    carries_no_skill(&frame(
        &json!(["cf", "codex-session", "/bin/codex"]),
        &json!({ "CF_CODEX_ROLE_FILE": "/c/developer-instructions.md" }),
    ))
    .unwrap();
    // The text the run judges is the part that follows what Codex gives every window.
    assert!(codex.contains("Specmint") && codex.contains("specmint-core"));
    let mine = ours(&codex);
    assert!(
        mine.starts_with("\n\nYour ConsensFlow role is chief."),
        "{mine}"
    );
    assert!(!mine.contains("specmint-core is installed"));
    assert!(has_the_section(mine));
    // Another harness's text is all ours.
    assert_eq!(ours(SECTION_TEXT), SECTION_TEXT);
    // A Codex window of no specs says nothing of them in the part that is ours.
    let off = "Codex's own: the human has Specmint installed.\n\nYour ConsensFlow role is chief. Nothing else.";
    assert!(told_nothing_of_specs(ours(off)).is_ok());
    assert!(told_nothing_of_specs(off).is_err());
}

/// A rollout of a Codex thread: its developer's message, and the user's messages.
fn rollout_of(developer: &str, user: &[&str]) -> String {
    let message = |role: &str, text: &str| {
        json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": role,
                "content": [{ "type": "input_text", "text": text }],
            },
        })
        .to_string()
    };
    std::iter::once(message("developer", developer))
        .chain(user.iter().map(|text| message("user", text)))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn a_window_opened_on_a_thread_again_names_it_after_resume() {
    let again = frame(
        &json!([
            "cf",
            "codex-session",
            "codex",
            "-c",
            "a=b",
            "resume",
            "01a09094-938f-7fd1-a2d3-315cf92b4559",
            "--model",
            "m"
        ]),
        &json!({}),
    );
    assert_eq!(
        thread_resumed(&again).as_deref(),
        Some("01a09094-938f-7fd1-a2d3-315cf92b4559")
    );
    let fresh = frame(
        &json!(["cf", "codex-session", "codex", "-c", "a=b"]),
        &json!({}),
    );
    assert_eq!(thread_resumed(&fresh), None);
    assert_eq!(thread_resumed(&frame(&json!(["resume"]), &json!({}))), None);
}

#[test]
fn a_chief_whose_record_has_the_section_in_what_its_thread_began_with_or_in_a_message_that_replaces_it_was_told(
) {
    // A thread that began with specs on has the section in its developer's message.
    let began_on = rollout_of(&format!("Codex's.\n\nYour role.{SECTION_TEXT}"), &["Hello"]);
    assert_eq!(
        told_in_the_record(&began_on).unwrap(),
        "in the developer instructions its thread began with"
    );
    // One that began with specs off keeps them, and is told by a message that says so.
    let message = format!(
        "[ConsensFlow m-9 · note from ConsensFlow]\nYour ConsensFlow role instructions have changed since this conversation began. The instructions below replace the ones you were given then: follow these from now on.\n\nThe role.{SECTION_TEXT}"
    );
    let told = rollout_of("Codex's.\n\nYour role.", &["Hello", &message]);
    assert_eq!(
        told_in_the_record(&told).unwrap(),
        "in a message of ConsensFlow's that replaces the instructions its thread began with"
    );
}

#[test]
fn a_chief_whose_thread_kept_what_it_began_with_and_was_told_nothing_was_not_told_and_the_report_says_what_its_record_has(
) {
    // The bug: specs on, closed and resumed, and the record has the one developer's
    // message of the start with specs off, and what the human typed.
    let kept = rollout_of("Codex's.\n\nYour role.", &["Write a spec for it."]);
    let failed = told_in_the_record(&kept).unwrap_err();
    assert_eq!(
        failed,
        "its record has no section of specs: 1 developer message and 1 user message"
    );
    let empty = told_in_the_record("").unwrap_err();
    assert_eq!(
        empty,
        "its record has no section of specs: 0 developer messages and 0 user messages"
    );
    // A message that has the section and does not say it replaces what the thread
    // began with is no way of telling a chief its role changed.
    let quoted = rollout_of("Codex's.", &[&format!("Look at this:{SECTION_TEXT}")]);
    let failed = told_in_the_record(&quoted).unwrap_err();
    assert!(failed.contains("does not say it replaces"), "{failed}");
}

#[test]
fn the_copy_the_chief_was_given_is_the_harness_s_own_where_it_has_the_variant_and_consensflow_s_where_it_has_not(
) {
    let own = "/home/rig/consensflow/integrations/specmint/claude/tdd-0123abcd/.claude/skills/specmint-tdd/SKILL.md";
    let global = "/home/me/.claude/skills/specmint-tdd/SKILL.md";
    let home = "/home/rig/consensflow";
    assert_eq!(
        which_copy("tdd", Some(own), Some("specmint-tdd"), None, home).unwrap(),
        format!("ConsensFlow's copy of specmint-tdd, at {own}")
    );
    assert_eq!(
        which_copy(
            "tdd",
            Some(global),
            Some("specmint-tdd"),
            Some(global),
            home
        )
        .unwrap(),
        format!("the harness's own copy of specmint-tdd, at {global}")
    );
    // The text writes a path with slashes on every system.
    assert!(which_copy(
        "tdd",
        Some("C:/Users/me/.claude/skills/specmint-tdd/SKILL.md"),
        Some("specmint-tdd"),
        Some(r"C:\Users\me\.claude\skills\specmint-tdd\SKILL.md"),
        home
    )
    .is_ok());
    for (named, frontmatter, installed, why) in [
        (None, None, None, "names no SKILL.md"),
        (
            Some(own),
            Some("specmint-core"),
            None,
            "is no copy of specmint-tdd",
        ),
        (Some(own), None, None, "is no copy of specmint-tdd"),
        (
            Some(own),
            Some("specmint-tdd"),
            Some(global),
            "has specmint-tdd installed at",
        ),
        (
            Some(global),
            Some("specmint-tdd"),
            None,
            "is not in the folder ConsensFlow keeps its copies in (/home/rig/consensflow/integrations/specmint/)",
        ),
        // A copy in the launch's own folder is the bug this was found in: the text
        // names another path at every launch.
        (
            Some("/home/rig/consensflow/integrations/codex/3d4e5f60-7182-4394-8dbe-2f3a4b5c6d70/role/.claude/skills/specmint-tdd/SKILL.md"),
            Some("specmint-tdd"),
            None,
            "is not in the folder ConsensFlow keeps its copies in",
        ),
    ] {
        let failed = which_copy("tdd", named, frontmatter, installed, home).unwrap_err();
        assert!(failed.contains(why), "{failed}");
    }
}

#[test]
fn what_each_harness_s_adapter_says_of_the_variant_is_what_the_judge_holds_the_chief_s_text_to() {
    use cf_e2e::live::{ask, Harness};

    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let consensflow = root.path().join("consensflow").display().to_string();
    let env = vec![
        ("HOME".to_owned(), home.display().to_string()),
        ("CONSENSFLOW_HOME".to_owned(), consensflow.clone()),
    ];
    for harness in Harness::ALL {
        // Nothing installed: ConsensFlow's own copy is where its text will name it.
        let none = ask::specmint(harness.kind(), "tdd", ".specs", &env).unwrap();
        assert!(!none.installed, "{harness:?}: {none:?}");
        let said = which_copy(
            "tdd",
            Some(&none.skill),
            Some("specmint-tdd"),
            None,
            &consensflow,
        )
        .unwrap();
        assert!(
            said.starts_with("ConsensFlow's copy of specmint-tdd"),
            "{harness:?}: {said}"
        );
        // The human's own, in the folder the harness reads, whatever it is called: Claude
        // Code reads its config folder's, the others the agents'.
        let skills = home
            .join(if harness == Harness::Claude {
                ".claude"
            } else {
                ".agents"
            })
            .join("skills")
            .join("tdd-by-the-human");
        std::fs::create_dir_all(&skills).unwrap();
        std::fs::write(
            skills.join("SKILL.md"),
            "---\nname: specmint-tdd\n---\nthe human's own\n",
        )
        .unwrap();
        let have = ask::specmint(harness.kind(), "tdd", ".specs", &env).unwrap();
        assert!(have.installed, "{harness:?}: {have:?}");
        assert!(
            have.skill.contains("tdd-by-the-human"),
            "{harness:?}: {have:?}"
        );
        // The human's own calls `.specs` the folder, and is never edited for one project: a
        // project on another folder is given ConsensFlow's copy, though the harness has this
        // (T-662).
        let other = ask::specmint(harness.kind(), "tdd", "docs/specs", &env).unwrap();
        assert!(!other.installed, "{harness:?}: {other:?}");
        let said = which_copy(
            "tdd",
            Some(&other.skill),
            Some("specmint-tdd"),
            None,
            &consensflow,
        )
        .unwrap();
        assert!(
            said.starts_with("ConsensFlow's copy of specmint-tdd"),
            "{harness:?}: {said}"
        );
        assert_ne!(other.skill, none.skill, "{harness:?}: a place of its own");
        let said = which_copy(
            "tdd",
            Some(&have.skill),
            Some("specmint-tdd"),
            Some(&have.skill),
            &consensflow,
        )
        .unwrap();
        assert!(
            said.starts_with("the harness's own copy of specmint-tdd"),
            "{harness:?}: {said}"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }
}

#[test]
fn a_copy_of_the_skill_names_the_project_s_folder_and_no_other() {
    let root = tempfile::tempdir().unwrap();
    let skill = root.path().join("specmint-core");
    std::fs::create_dir_all(skill.join("references")).unwrap();
    // The vendored copy: the folder the skill's files call canonical.
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: specmint-core\n---\nRegistry: `.specs/registry.md`\n",
    )
    .unwrap();
    std::fs::write(
        skill.join("references/spec-format.md"),
        "Per-spec files: `.specs/<id>/SPEC.md`\n",
    )
    .unwrap();
    // For a project that keeps its specs in `.specs` there is no other to name.
    assert!(names_the_folder_alone(&skill, ".specs").is_ok());
    // For any other it is the field's failure: the copy sends the chief to `.specs`.
    let failed = names_the_folder_alone(&skill, "docs/specs").unwrap_err();
    assert!(failed.contains("still says .specs"), "{failed}");
    // The copy ConsensFlow writes for the folder: every `.specs` is the folder.
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: specmint-core\n---\nRegistry: `docs/specs/registry.md`\n",
    )
    .unwrap();
    let failed = names_the_folder_alone(&skill, "docs/specs").unwrap_err();
    assert!(
        failed.contains("spec-format.md") && failed.contains("still says .specs"),
        "a file under references says it too: {failed}"
    );
    std::fs::write(
        skill.join("references/spec-format.md"),
        "Per-spec files: `docs/specs/<id>/SPEC.md`\n",
    )
    .unwrap();
    let said = names_the_folder_alone(&skill, "docs/specs").unwrap();
    assert!(
        said.contains("2 files") && said.contains("docs/specs"),
        "{said}"
    );
    // A copy that names no folder at all is no copy of the project's.
    std::fs::write(skill.join("SKILL.md"), "---\nname: specmint-core\n---\n").unwrap();
    assert!(names_the_folder_alone(&skill, "docs/specs").is_err());
}
