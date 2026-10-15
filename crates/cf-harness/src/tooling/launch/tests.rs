//! The Devin window of a member, asked of the adapter as the live tests ask it:
//! the question as JSON, the launch as JSON. Devin's version is scripted, as the
//! adapter's own tests have it, so no program runs here.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::path;
use serde_json::{json, Value};

use super::*;
use crate::testing::{called, fake_executable};
use crate::tooling::answer;

const LAUNCH: &str = "00000000-0000-4000-8000-0000000000d1";
const MESSAGE: &str = "[ConsensFlow m-1 · T-1 · task from @chief]\nWrite the parser";
const ROLE: &str = "# ConsensFlow worker\nRun cf ask to ask the chief.";
const DEVIN: &str = "devin 3000.11.3 (9c803229faa4)";

/// A home of its own, with a stand-in `devin` on its `PATH` that the fakes say
/// `version` for.
struct Home {
    dir: tempfile::TempDir,
    env: Env,
    fakes: Fakes,
}

impl Home {
    fn new(version: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let at = |name: &str| path::join(&[&root, name]);
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        let devin = fake_executable(&dir.path().join("bin").join("devin"));
        let env = Env::from_vars([
            ("HOME", at("home")),
            ("CONSENSFLOW_HOME", at("consensflow")),
            ("XDG_CONFIG_HOME", at("config")),
            ("PATH", at("bin")),
        ]);
        let fakes = Fakes::new(&env);
        let line_end = if cfg!(windows) { "\r\n" } else { "\n" };
        fakes
            .processes
            .every_answer(&called(&devin), Ok(format!("{version}{line_end}")));
        Self { dir, env, fakes }
    }

    fn at(&self, parts: &[&str]) -> PathBuf {
        parts
            .iter()
            .fold(self.dir.path().to_path_buf(), |folder, part| {
                folder.join(part)
            })
    }

    /// The launch's own folder, where its files are written.
    fn folder(&self) -> PathBuf {
        self.at(&["consensflow", "integrations", "devin", LAUNCH])
    }

    /// The question of a member's launch in this home, with `fields` over it.
    fn asked(&self, fields: Value) -> Value {
        let mut asked = json!({
            "op": "devin_launch",
            "launch": LAUNCH,
            "role": "worker",
            "directory": self.at(&["work"]),
            "message": MESSAGE,
            "agent": { "kind": "devin", "model": "swe-2", "effort": "medium" },
            "instructions": ROLE,
            "cf": self.at(&["bundle", "bin", "cf"]),
            "env": {},
        });
        for (name, value) in fields.as_object().into_iter().flatten() {
            asked[name] = value.clone();
        }
        asked
    }

    /// The question prepared by the adapter over this home's fakes.
    fn launch(&self, fields: Value) -> Result<Value, String> {
        let asked = self.asked(fields);
        let cf = self.at(&["bundle", "bin", "cf"]);
        let services = Services {
            bundle: bundle_of(&cf),
            ..self.fakes.services(&self.env, Path::new(""))
        };
        prepared(&asked, &services)
    }

    /// The launch's own config, as it was written.
    fn config(&self) -> Value {
        let text = fs::read_to_string(self.folder().join("config.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

fn words(launched: &Value) -> Vec<String> {
    launched["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_owned())
        .collect()
}

fn shown(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn a_members_first_launch_is_its_own_config_the_flags_of_a_full_permission_window_and_its_task_in_a_prompt_file(
) {
    let home = Home::new(DEVIN);
    let launched = home.launch(json!({})).unwrap();
    let folder = home.folder();
    // The program the adapter found, which the window opens on.
    assert!(launched["command"].as_str().unwrap().contains("devin"));
    assert_eq!(
        words(&launched),
        [
            "--config",
            &shown(&folder.join("config.json")),
            "--model",
            "swe-2-medium",
            "--permission-mode",
            "dangerous",
            "--respect-workspace-trust",
            "false",
            "--prompt-file",
            &shown(&folder.join("prompt.txt")),
        ]
    );
    assert_eq!(
        fs::read_to_string(folder.join("prompt.txt")).unwrap(),
        MESSAGE
    );
    // Devin names its own conversation.
    assert_eq!(launched["session"], Value::Null);
    // What the launch adds to the window's variables: its wire log and its role.
    assert_eq!(
        launched["env"]["CHISEL_PURE_ACP_WIRE_LOG"],
        json!(shown(&folder.join("wire.jsonl")))
    );
    let role = launched["env"]["CF_DEVIN_ROLE_FILE"].as_str().unwrap();
    assert!(fs::read_to_string(role).unwrap().starts_with(ROLE));
    assert_eq!(launched["dropEnv"], json!([]));
}

#[test]
fn the_config_has_the_hook_that_gives_a_session_its_role_and_a_members_question_hook() {
    let home = Home::new(DEVIN);
    home.launch(json!({})).unwrap();
    let config = home.config();
    let cf = shown(&home.at(&["bundle", "bin", "cf"])).replace('\\', "/");
    assert_eq!(
        config["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        json!(format!("'{cf}' hook devin-session"))
    );
    assert_eq!(
        config["hooks"]["PreToolUse"][0]["matcher"],
        "ask_user_question"
    );
    assert_eq!(
        config["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "cf hook devin"
    );
    assert_eq!(config["auto_update"], false);
}

#[test]
fn a_chiefs_question_tool_is_left_to_devins_own_dialog() {
    let home = Home::new(DEVIN);
    home.launch(json!({ "role": "chief" })).unwrap();
    assert_eq!(home.config()["hooks"]["PreToolUse"], json!([]));
}

#[test]
fn a_resumed_launch_names_its_session_and_a_note_it_is_given_goes_in_a_prompt_file() {
    let note = "[ConsensFlow m-2 · T-1 · note from ConsensFlow]\nGo on where you stopped.";
    let home = Home::new(DEVIN);
    let launched = home
        .launch(json!({ "resume": "mild-coin", "message": note }))
        .unwrap();
    let folder = home.folder();
    let args = words(&launched);
    assert_eq!(
        args[..4],
        [
            "--config",
            &shown(&folder.join("config.json")),
            "--resume",
            "mild-coin"
        ]
    );
    assert_eq!(
        args[args.len() - 2..],
        ["--prompt-file", &shown(&folder.join("prompt.txt"))]
    );
    assert_eq!(fs::read_to_string(folder.join("prompt.txt")).unwrap(), note);
    assert_eq!(launched["session"], "mild-coin");
}

#[test]
fn a_resumed_launch_with_no_message_has_no_prompt_file() {
    let home = Home::new(DEVIN);
    let launched = home
        .launch(json!({ "resume": "mild-coin", "message": null }))
        .unwrap();
    assert!(!words(&launched).contains(&"--prompt-file".to_owned()));
    assert!(!home.folder().join("prompt.txt").exists());
}

#[test]
fn a_launch_is_made_once_and_a_devin_that_is_too_old_is_refused() {
    let home = Home::new(DEVIN);
    home.launch(json!({})).unwrap();
    // The folder of a launch holds its config, which is written once: a window that
    // is opened again is another launch.
    let again = home.launch(json!({})).unwrap_err();
    assert!(again.contains("EEXIST"), "{again}");
    let old = Home::new("devin 3000.9.1 (00000000)");
    let refused = old.launch(json!({})).unwrap_err();
    assert!(refused.contains("3000.10.21 or newer"), "{refused}");
    assert!(!old.folder().exists(), "nothing was written");
}

#[test]
fn a_launch_that_is_not_one_or_is_missing_a_part_of_what_it_needs_says_which() {
    let home = Home::new(DEVIN);
    // A question with the field `name` taken out.
    let without = |name: &str| {
        let mut asked = home.asked(json!({}));
        asked.as_object_mut().unwrap().remove(name);
        answer(&asked).unwrap_err()
    };
    assert_eq!(
        answer(&home.asked(json!({ "launch": "NOT-A-LAUNCH" }))).unwrap_err(),
        "the question's launch is not a launch id (a lowercase uuid)"
    );
    // A uuid in capitals is no launch id either: a folder is named by it.
    let capitals = home.asked(json!({ "launch": LAUNCH.to_uppercase() }));
    assert!(answer(&capitals).unwrap_err().contains("not a launch id"));
    for name in [
        "launch",
        "role",
        "directory",
        "instructions",
        "cf",
        "env",
        "agent",
    ] {
        let failed = without(name);
        assert!(failed.starts_with("the question names no "), "{failed}");
        assert!(failed.contains(name), "{failed}");
    }
}
