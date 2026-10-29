//! A resumed Codex thread kept the developer instructions it began with.
//!
//! Seen by the owner on 2026-10-13, on 3.0.3 and Codex 0.162.1, by the live specs
//! check (`cargo xtask live specs codex`). The human turns specs on for a project
//! and closes and resumes it, which is how a change of the setting reaches a chief,
//! and every other harness's chief was told of them. Codex's never was: it found
//! the owner's own Specmint skill by itself, used the folder Specmint names and
//! not the project's, and gave the writing of the spec to a worker as a task, again
//! and again. ConsensFlow gave a Codex window its role as `-c
//! developer_instructions=…` at each launch (since T-698, a file the window's broker
//! gives a thread as it starts), and a resumed thread keeps the developer message it
//! began with: the chief's record held the one message of its
//! start with specs off, and none after the Resume. So any change of a role since a
//! Codex thread began never reached it. The report is the issue
//! `issues/codex-resume-keeps-first-instructions.md` in consensflow-sme.
//!
//! What the owner should have seen: the chief's own record holds the section of the
//! project's specs after the Resume, said in a message that tells it to follow the
//! new instructions in place of the ones it began with, naming the folder the
//! project says and the skill the chief can read; and a chief resumed with nothing
//! changed is told nothing more.
//!
//! The first fix did not hold that last part where Codex had no Specmint of its
//! own (found in review, the same day): ConsensFlow's copy of the skill was in the
//! launch's folder, so the role text named a different path at every launch, and a
//! chief with specs on was told its whole role again at every Resume. The copy is
//! kept now at one place for every launch (`integrations/specmint/codex/<variant>-<commit>`),
//! and the role text is the same until something changed: the second Resume says
//! nothing, and a change of the variant says it again, once.
//!
//! The review of that fix found the note was the launch's first message
//! (2026-10-14): the role was then on the window's command line twice, which
//! Windows has no room for with a large staff, and the message the launch would
//! have begun with lost its place. (Since T-698 the role is on no command line.) The window opens with no first message now, and
//! the note goes in by paste once it is up, ahead of everything else.

use std::fs;
use std::path::{Path, PathBuf};

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::rollout;
use regex::Regex;
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// What the section of specs in a chief's text begins with.
const SECTION: &str = "## This project's specs";

/// What a message that gives a role again says, which no other note does.
const REPLACES: &str = "replace the ones you were given then";

/// The folder the project keeps its specs in, below its own.
const FOLDER: &str = "docs/specs";

/// The chief of a project, a Codex agent, after a turn of its own, and where its
/// rollout is: the turn is what gives its thread the instructions it began with,
/// and what lets Codex resume it.
struct Chief<'a> {
    rig: &'a Rig,
    project: Project<'a>,
    home: PathBuf,
}

impl<'a> Chief<'a> {
    /// A project whose chief is a Codex agent, with a worker on its staff from the
    /// start (a member added later is a change of the chief's text); the chief puts
    /// a task on the board, and its turn is the worker's result coming to it.
    fn after_a_turn(rig: &'a Rig) -> Result<Self, Box<dyn std::error::Error>> {
        let project = Project::open_with_staff(rig, "maia", None, &[("worker", "worker")])?;
        let window = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
        let tiers = project.tiers()?;
        let tier = tiers.get("worker").map_or("standard", String::as_str);
        let ran = rig.cf_in_window(
            &window,
            [
                "task",
                "add",
                "--tier",
                tier,
                "Reply with exactly: WORKER_OK",
            ],
        )?;
        assert_eq!(ran.code, Some(0), "{ran}");
        let chief = Self {
            rig,
            project,
            home: PathBuf::from(rig.var("CODEX_HOME").unwrap_or_default()),
        };
        // The result is in its thread and the daemon has seen it arrive: a window that
        // is closed before it has is given the message again.
        rig.wait_for("the worker's result to reach the chief", secs(60), || {
            let arrived = rollout::said_by(&chief.record()?, "user")
                .iter()
                .any(|said| said.contains("WORKER_OK"));
            let delivered = chief
                .project
                .inbox("chief")?
                .iter()
                .any(|message| message["kind"] == "result" && message["state"] == "delivered");
            Ok(arrived && delivered)
        })?;
        chief.waits_to_be_idle()?;
        Ok(chief)
    }

    /// What the chief's thread has recorded: the one rollout of the rig's Codex home.
    fn record(&self) -> Result<String, cf_e2e::Error> {
        match rollout::rollouts(&self.home).as_slice() {
            [only] => rollout::read(only),
            others => Ok(format!("{} rollouts", others.len())),
        }
    }

    fn waits_to_be_idle(&self) -> Result<(), cf_e2e::Error> {
        self.rig.wait_for("the chief to be idle", secs(60), || {
            Ok(self.project.lane("chief")?["activity"]["state"] == "idle")
        })
    }

    /// The human turns the project's specs on, in `FOLDER`, with the variant
    /// `variant`.
    fn is_given_specs(&self, variant: &str) -> Result<(), Box<dyn std::error::Error>> {
        let said = self.rig.page(
            "project.specs",
            json!({ "project": self.project.id(), "on": true, "folder": FOLDER, "variant": variant }),
        )?;
        assert_eq!(said["ok"], true, "{said}");
        Ok(())
    }

    /// The human closes the project and resumes it, as the live check does, and the
    /// window the chief's next launch opens is waited for: `windows` of them by now.
    fn is_resumed(&self, windows: usize) -> Result<Value, Box<dyn std::error::Error>> {
        let id = self.project.id();
        for op in ["project.close", "project.resume"] {
            let done = self.rig.page(op, json!({ "project": id }))?;
            assert_eq!(done["ok"], true, "{op}: {done}");
        }
        let chief = format!("p{id}-chief");
        let opened = || {
            self.rig
                .open_frames()
                .into_iter()
                .filter(|frame| frame["id"] == chief.as_str())
                .collect::<Vec<_>>()
        };
        self.rig.wait_for("the chief's next window", secs(60), || {
            Ok(opened().len() >= windows)
        })?;
        Ok(opened().remove(windows - 1))
    }

    /// How many notes the daemon wrote to tell the chief its role again. A launch
    /// writes one before its window opens, so this says at once whether a Resume
    /// had anything to tell: the chief's own record would say it only once the
    /// window had been given the note, which is after the chief reads idle.
    fn notes_of_the_role(&self) -> Result<usize, cf_e2e::Error> {
        Ok(self
            .project
            .inbox("chief")?
            .iter()
            .filter(|message| message["body"].to_string().contains(REPLACES))
            .count())
    }

    /// The user messages of the chief's thread that hold the section of specs.
    fn told_of_specs(&self) -> Result<Vec<String>, cf_e2e::Error> {
        Ok(rollout::said_by(&self.record()?, "user")
            .into_iter()
            .filter(|said| said.contains(SECTION))
            .collect())
    }
}

/// The file the section names as its skill's `SKILL.md`, in the message that told it.
fn skill_named_in(message: &str) -> Option<String> {
    Regex::new(r"its SKILL\.md is at (.+?), and the files it names are beside it")
        .ok()?
        .captures(message)
        .map(|found| found[1].to_owned())
}

/// The folder the message says Specmint's `.specs` means.
fn folder_named_in(message: &str) -> Option<String> {
    Regex::new(r"Where Specmint says `\.specs`, it means `([^`]+)`")
        .ok()?
        .captures(message)
        .map(|found| found[1].to_owned())
}

#[test]
fn a_codex_chief_resumed_with_its_specs_turned_on_has_the_section_in_its_own_record_and_the_skill_it_names_is_there(
) -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    let chief = Chief::after_a_turn(&rig)?;
    let before = chief.record()?;
    assert!(
        rollout::said_by(&before, "developer")
            .iter()
            .all(|said| !said.contains(SECTION)),
        "its thread began with specs off"
    );
    assert!(chief.told_of_specs()?.is_empty());

    chief.is_given_specs("tdd")?;
    let resumed = chief.is_resumed(2)?;
    // The window's command line has nothing of the role: its role's file holds the
    // section (and is given to no resumed thread), and the note that says it again
    // goes in by paste once the window is up.
    let line = resumed["argv"].to_string();
    assert_eq!(line.matches(SECTION).count(), 0, "{line}");
    assert!(!line.contains(REPLACES), "no message of a role: {line}");
    let file = resumed["env"]["CF_CODEX_ROLE_FILE"]
        .as_str()
        .ok_or("the window's environment names no role file")?;
    assert_eq!(fs::read_to_string(file)?.matches(SECTION).count(), 1);
    rig.wait_for(
        "the chief's own record to hold the section",
        secs(60),
        || Ok(!chief.told_of_specs()?.is_empty()),
    )?;

    // What it was told, and where: the folder of the project, not Specmint's, and a
    // skill the chief can read while its window is up.
    let told = chief.told_of_specs()?;
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(
        told[0].contains(REPLACES),
        "it says it is in place of the instructions it began with: {}",
        told[0]
    );
    assert_eq!(folder_named_in(&told[0]).as_deref(), Some(FOLDER));
    let skill = skill_named_in(&told[0]).ok_or("the message names no SKILL.md")?;
    assert!(Path::new(&skill).is_file(), "{skill}");
    assert!(
        Path::new(&skill)
            .parent()
            .is_some_and(|folder| folder.join("references").is_dir()),
        "the files the skill names are beside it"
    );
    assert!(
        fs::read_to_string(&skill)?.starts_with("---\nname: specmint-tdd\n"),
        "ConsensFlow's copy of the variant the project chose"
    );
    // The instructions it began with are the ones its thread keeps: no section there.
    assert_eq!(
        rollout::said_by(&chief.record()?, "developer"),
        rollout::said_by(&before, "developer")
    );

    // Resumed again with nothing changed in the setting, it is told nothing: the
    // role names the skill where it was told it is, the copy of it outlasting the
    // window that was told, and its first message is none of a role's.
    chief.waits_to_be_idle()?;
    let second = chief.is_resumed(3)?;
    assert!(
        !second["argv"].to_string().contains(REPLACES),
        "no message of a role: {}",
        second["argv"]
    );
    assert_eq!(
        chief.notes_of_the_role()?,
        1,
        "the Resume wrote nothing to tell it the role again"
    );
    chief.waits_to_be_idle()?;
    assert_eq!(chief.told_of_specs()?, told, "told once");
    assert!(Path::new(&skill).is_file(), "{skill} is where it was");

    // The human chooses another variant: that is a change, said again, once, and the
    // role names the copy of that variant, kept beside the first.
    chief.is_given_specs("core")?;
    chief.is_resumed(4)?;
    rig.wait_for(
        "the chief to be told of the other variant",
        secs(60),
        || Ok(chief.told_of_specs()?.len() == 2),
    )?;
    let again = chief.told_of_specs()?;
    let now = skill_named_in(&again[1]).ok_or("the second message names no SKILL.md")?;
    assert_ne!(now, skill);
    assert!(now.contains("specmint-core"), "{now}");
    assert!(Path::new(&now).is_file(), "{now}");
    assert!(Path::new(&skill).is_file(), "{skill} is left alone");
    chief.waits_to_be_idle()?;
    chief.is_resumed(5)?;
    assert_eq!(
        chief.notes_of_the_role()?,
        2,
        "and the Resume after it wrote none"
    );
    chief.waits_to_be_idle()?;
    assert_eq!(chief.told_of_specs()?.len(), 2, "and no more than that");
    rig.close()?;
    Ok(())
}

#[test]
fn a_codex_chief_resumed_with_its_specs_off_is_told_nothing_more() -> Outcome {
    let rig = Rig::start(config_with_codex())?;
    let chief = Chief::after_a_turn(&rig)?;
    let before = rollout::said_by(&chief.record()?, "user");
    chief.is_resumed(2)?;
    assert_eq!(
        chief.notes_of_the_role()?,
        0,
        "a Resume where the role did not change wrote nothing to say it"
    );
    chief.waits_to_be_idle()?;
    chief.is_resumed(3)?;
    assert_eq!(chief.notes_of_the_role()?, 0, "nor did the next");
    chief.waits_to_be_idle()?;
    assert_eq!(
        rollout::said_by(&chief.record()?, "user"),
        before,
        "nothing was said to a thread whose role did not change"
    );
    assert!(chief.told_of_specs()?.is_empty());
    rig.close()?;
    Ok(())
}
