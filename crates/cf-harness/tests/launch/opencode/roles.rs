//! The role an OpenCode window is given, as the OpenCode cases of Node's
//! role-skills suite held them: its whole text, in a file of the launch's own
//! and in the configuration OpenCode reads from its environment, merged into
//! the one the human set.

use std::fs;
use std::path::Path;

use cf_harness::contract::{Adapter, LaunchId, Prepared, Skill};
use cf_harness::testing::unrooted;
use cf_proto::specs::Variant;
use serde_json::{json, Value};

use super::stage::{planned, Home, Stage, Wanted, LAUNCH};

/// The `OPENCODE_CONFIG_CONTENT` a plan gives the window, as JSON.
fn configuration(plan: &Prepared) -> Value {
    serde_json::from_str(planned(plan, "OPENCODE_CONFIG_CONTENT").unwrap()).unwrap()
}

/// A stage whose environment holds the configuration `config`.
fn stage_with(config: &str) -> Stage {
    Stage::with(Home::new().sharing("OPENCODE_CONFIG_CONTENT", config), None)
}

#[test]
fn every_role_enters_opencode_with_its_whole_text_already_loaded() {
    let original = json!({
        "theme": "user",
        "skills": { "paths": ["/user/skills"], "urls": ["https://example.com/skills"] },
        "instructions": ["/user/rules.md"],
    });
    for name in [
        "chief", "advisor", "worker", "reviewer", "tester", "designer",
    ] {
        let stage = stage_with(&original.to_string());
        stage.serves_creation("ses_abc123");
        let content = format!(
            "The {name}'s whole text: \"quotes\", `backticks`, $HOME\nand newlines.\n| saved-worker |"
        );
        let request = Wanted {
            instructions: content.clone(),
            role: name,
            ..Wanted::default()
        };
        let plan = stage.prepare(&request).unwrap();
        let merged = configuration(&plan);
        let file = merged["instructions"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            file.replace('\\', "/")
                .ends_with(&format!("/consensflow-{name}/SKILL.md")),
            "{file}"
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            content,
            "the whole role text is loaded"
        );
        assert_eq!(merged["instructions"], json!(["/user/rules.md", file]));
        assert_eq!(merged["theme"], original["theme"]);
        assert_eq!(merged["skills"]["urls"], original["skills"]["urls"]);
        assert_eq!(merged["skills"]["paths"][0], original["skills"]["paths"][0]);
        // The window's configuration, given to it again, merges to itself.
        let given = planned(&plan, "OPENCODE_CONFIG_CONTENT").unwrap();
        let again = Stage::with(stage.home.sharing("OPENCODE_CONFIG_CONTENT", given), None);
        again.serves_creation("ses_abc123");
        assert_eq!(configuration(&again.prepare(&request).unwrap()), merged);
        // What the engine runs with is not edited: the plan is what it adds.
        assert_eq!(
            stage.home.env().text("OPENCODE_CONFIG_CONTENT"),
            Some(&*original.to_string())
        );
    }
}

#[test]
fn each_launch_has_a_role_file_of_its_own_one_chief_never_reads_another_projects_staff() {
    let stage = Stage::new();
    let file = |staff: &str| {
        stage.serves_creation("ses_abc123");
        let request = Wanted {
            instructions: format!("# Chief\n| {staff} |"),
            role: "chief",
            ..Wanted::default()
        };
        let plan = stage.prepare(&request).unwrap();
        let merged = configuration(&plan);
        merged["instructions"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (a, b) = (file("saved-worker"), file("nobody"));
    assert!(a.starts_with(&stage.home.launch_folder()), "{a}");
    assert_eq!(a, b, "one launch's file is its own, written again");
    assert_eq!(fs::read_to_string(&b).unwrap(), "# Chief\n| nobody |");
    assert!(!fs::read_to_string(&b).unwrap().contains("saved-worker"));
}

#[test]
fn opencode_rejects_malformed_instruction_lists_before_native_launch() {
    for instructions in ["\"rules.md\"", "null", "{}", "7", "[\"rules.md\", 7]"] {
        let stage = stage_with(&format!("{{\"instructions\":{instructions}}}"));
        stage.serves_creation("ses_abc123");
        let request = Wanted {
            role: "chief",
            ..Wanted::default()
        };
        assert_eq!(
            stage.prepare(&request).err().as_deref(),
            Some("OpenCode instructions must be an array of paths"),
            "{instructions}"
        );
        // Before any server was started for it.
        assert!(stage.started.started.borrow().is_empty(), "{instructions}");
    }
}

#[test]
fn a_window_without_its_role_text_is_refused_and_a_role_text_is_private_to_its_launch() {
    let stage = Stage::new();
    let refused = stage.prepare(&Wanted {
        instructions: String::new(),
        ..Wanted::default()
    });
    assert_eq!(
        refused.err().as_deref(),
        Some("the worker window needs its role text")
    );
    assert!(!Path::new(&stage.home.launch_folder()).exists());
    stage.serves_creation("ses_abc123");
    let plan = stage.prepare(&Wanted {
        instructions: "worker text".to_owned(),
        ..Wanted::default()
    });
    let merged = configuration(&plan.unwrap());
    let file = merged["instructions"][0].as_str().unwrap();
    assert_eq!(fs::read_to_string(file).unwrap(), "worker text");
    assert!(file.starts_with(&stage.home.launch_folder()));
    // Windows has no POSIX modes; its files answer 0o666 whatever the writer asked.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

/// The names in the skills folder OpenCode is told to read, in a plan's configuration.
fn skills_of(plan: &Prepared) -> Vec<String> {
    let merged = configuration(plan);
    let folder = merged["skills"]["paths"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_chief_given_specs_has_the_skill_written_where_the_role_s_is_which_opencode_is_told_to_read_and_its_configuration_is_as_it_was(
) {
    for variant in Variant::ALL {
        // Two stages of one make: what differs between the launches is the specs.
        let (off_stage, stage) = (Stage::new(), Stage::new());
        off_stage.serves_creation("ses_abc123");
        stage.serves_creation("ses_abc123");
        let chief = Wanted {
            role: "chief",
            message: None,
            ..Wanted::default()
        };
        let off = off_stage.prepare(&chief).unwrap();
        assert_eq!(
            skills_of(&off),
            ["consensflow-chief"],
            "with specs off the role is alone"
        );
        let on = stage
            .prepare(&Wanted {
                specs: Some(variant),
                ..chief
            })
            .unwrap();
        assert_eq!(
            unrooted(&on, &stage.home.root),
            unrooted(&off, &off_stage.home.root),
            "{variant:?}: OpenCode reads the same folder, and is told nothing more"
        );
        assert_eq!(
            skills_of(&on),
            ["consensflow-chief", variant.skill_name()],
            "{variant:?}"
        );
    }
}

#[test]
fn an_opencode_that_has_the_variant_installed_in_any_place_it_reads_is_given_nothing_of_ours_and_another_variant_installed_is_left_alone(
) {
    let stage = Stage::new();
    let install = |folder: &str, name: &str| {
        let dir = format!("{}/home/{folder}/{name}", stage.home.root);
        fs::create_dir_all(&dir).unwrap();
        let file = format!("{dir}/SKILL.md");
        fs::write(&file, format!("---\nname: {name}\n---\nthe human's own\n")).unwrap();
        file
    };
    let places = [
        (
            Variant::Core,
            install(".config/opencode/skills", "specmint-core"),
        ),
        (
            Variant::CoreHtml,
            install(".config/opencode/skill", "specmint-core-html"),
        ),
        (Variant::Tdd, install(".claude/skills", "specmint-tdd")),
        (
            Variant::TddHtml,
            install(".agents/skills", "specmint-tdd-html"),
        ),
    ];
    let id = LaunchId::new(LAUNCH).unwrap();
    for (variant, file) in &places {
        assert_eq!(
            stage.adapter.specmint(*variant, &id).unwrap(),
            Skill::Global(file.clone())
        );
        stage.serves_creation("ses_abc123");
        let chief = Wanted {
            role: "chief",
            message: None,
            specs: Some(*variant),
            ..Wanted::default()
        };
        let plan = stage.prepare(&chief).unwrap();
        assert_eq!(
            skills_of(&plan),
            ["consensflow-chief"],
            "{variant:?}: nothing is written"
        );
    }
    assert!(places.iter().all(|(_, file)| fs::read_to_string(file)
        .unwrap()
        .ends_with("the human's own\n")));
}
