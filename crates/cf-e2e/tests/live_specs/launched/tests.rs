use serde_json::json;

use super::*;

const SECTION_TEXT: &str = "\n## This project's specs\n\nSpecs are on in this project. Plan work that is big enough for phases as a spec, with the Specmint skill `specmint-tdd`: its SKILL.md is at /home/me/.consensflow/integrations/claude/abc/role/.claude/skills/specmint-tdd/SKILL.md, and the files it names are beside it; read it before you plan. Where Specmint says `.specs`, it means `plans/specs`: your specs and their registry are there.\n";

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
            "/home/me/.consensflow/integrations/claude/abc/role/.claude/skills/specmint-tdd/SKILL.md"
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
    // Codex: the developer's instructions, a JSON string in a `-c` setting.
    assert_eq!(
        role_of(&frame(
            &json!([
                "cf",
                "codex-session",
                "-c",
                r#"developer_instructions="line one\n\nline \"two\"""#
            ]),
            &json!({})
        )),
        Some(RoleText::Inline("line one\n\nline \"two\"".to_owned()))
    );
    // Devin: a file its session-start hook reads, named in its environment.
    assert_eq!(
        role_of(&frame(
            &json!(["devin"]),
            &json!({ "CF_DEVIN_ROLE_FILE": "/d/role" })
        )),
        Some(RoleText::File("/d/role".to_owned()))
    );
    // OpenCode: the last of the instructions its configuration lists.
    assert_eq!(
        role_of(&frame(
            &json!(["opencode"]),
            &json!({ "OPENCODE_CONFIG_CONTENT": r#"{"instructions":["/user/rules.md","/o/role"]}"# })
        )),
        Some(RoleText::File("/o/role".to_owned()))
    );
    // A window given its role in none of these ways is one the run cannot read.
    assert_eq!(role_of(&frame(&json!(["claude"]), &json!({}))), None);
    assert_eq!(
        role_of(&frame(
            &json!(["opencode"]),
            &json!({ "OPENCODE_CONFIG_CONTENT": "{}" })
        )),
        None
    );
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
        &json!(["opencode"]),
        &json!({ "OPENCODE_CONFIG_CONTENT": r#"{"skills":{"paths":["/r/specmint-tdd"]}}"# }),
    );
    let failed = carries_no_skill(&in_env).unwrap_err();
    assert!(
        failed.starts_with(
            "the window's environment names a Specmint skill: OPENCODE_CONFIG_CONTENT="
        ),
        "{failed}"
    );
}

#[test]
fn what_codex_says_of_the_human_s_own_skills_is_no_skill_of_the_window_and_no_text_of_ours() {
    let codex = format!(
        "Codex's own words, and the human's: a skill called specmint-core is installed.\n\nYour ConsensFlow role is chief. The following role instructions are already loaded.{SECTION_TEXT}"
    );
    let setting = format!(
        "developer_instructions={}",
        serde_json::to_string(&codex).unwrap()
    );
    carries_no_skill(&frame(
        &json!(["cf", "codex-session", "-c", setting]),
        &json!({}),
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

#[test]
fn the_copy_the_chief_was_given_is_the_harness_s_own_where_it_has_the_variant_and_consensflow_s_where_it_has_not(
) {
    let own =
        "/home/rig/consensflow/integrations/claude/abc/role/.claude/skills/specmint-tdd/SKILL.md";
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
            "is not under /home/rig/consensflow",
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
        let none = ask::specmint(harness.kind(), "tdd", &env).unwrap();
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
        let have = ask::specmint(harness.kind(), "tdd", &env).unwrap();
        assert!(have.installed, "{harness:?}: {have:?}");
        assert!(
            have.skill.contains("tdd-by-the-human"),
            "{harness:?}: {have:?}"
        );
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
