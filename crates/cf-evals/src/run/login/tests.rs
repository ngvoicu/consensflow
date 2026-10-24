//! The throwaway Claude config folder of an eval: when a run has one, and what it keeps for the
//! report instead of failing the rig.

use std::sync::atomic::{AtomicUsize, Ordering};

use cf_e2e::Error;

use super::*;
use crate::plan::staff_for;

/// A config folder that says `verdict` each time the rig that used it ends, and counts what the rig
/// asked of it.
#[derive(Debug)]
struct Says {
    verdict: Mutex<Vec<Option<&'static str>>>,
    kept: AtomicUsize,
}

impl Says {
    fn once(verdicts: Vec<Option<&'static str>>) -> Self {
        Self {
            verdict: Mutex::new(verdicts),
            kept: AtomicUsize::new(0),
        }
    }
}

impl ClaudeConfig for Says {
    fn folder(&self) -> Support<PathBuf> {
        Ok(PathBuf::from("/tmp/the-folder"))
    }

    fn described(&self) -> String {
        "the test".to_owned()
    }

    fn keep_records(&self, _into: &Path) -> Support {
        self.kept.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn finish(&self) -> Support {
        match self.verdict.lock().unwrap().remove(0) {
            Some(said) => Err(Error::Daemon(said.to_owned())),
            None => Ok(()),
        }
    }
}

/// What the kit's login says when the run touched the owner's setup and refreshed the copy.
const BANNER: &str = "\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!\n\
    THE RUN CHANGED THE OWNER'S CLAUDE SETUP, which a live run on its own login is never to touch:\n  \
    - ~/.claude/settings.json is not as it was\n  \
    - ~/.claude/history.jsonl gained 2 prompt(s) typed in a folder of the run\n\
    THE RUN'S CLAUDE CODE REFRESHED ITS COPY OF THE LOGIN. A refresh token is used once:\n  \
    - the run's Claude Code wrote a different login in the folder's .credentials.json\n\
    !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!";

fn staff_on(harnesses: &[&str], models: &[(&str, &str)]) -> Staff {
    staff_for(harnesses, models, None).unwrap()
}

#[test]
fn a_run_has_a_real_claude_code_unless_every_one_of_its_windows_is_the_stand_in_agent() {
    let on = |chief: &str, model: &str, harnesses: &[&str], staff_model: &str| {
        real_claude(
            chief,
            model,
            &staff_on(harnesses, &[("claude", staff_model)]),
        )
    };
    // The dry runs: the stand-in's models, chief and staff.
    assert!(!on("claude", "fake-chief", &["claude"], "fake"));
    // Real: the chief's model, or a member's, or both.
    assert!(on(
        "claude",
        "claude-opus-5",
        &["claude"],
        "claude-haiku-4-5-20251001"
    ));
    assert!(on("claude", "claude-opus-5", &["claude"], "fake"));
    assert!(on(
        "claude",
        "fake-chief",
        &["claude"],
        "claude-haiku-4-5-20251001"
    ));
    // A chief that is not Claude Code, on a staff that is the stand-in: none of Claude is real.
    assert!(!on("codex", "gpt", &["claude"], "fake"));
    // ... and on a real staff of Claude Code it is.
    assert!(on("codex", "gpt", &["claude"], "claude-haiku-4-5-20251001"));
    // No Claude Code at all.
    assert!(!on("codex", "gpt", &["codex"], "fake"));
}

#[test]
fn the_login_must_last_the_runs_planned_time_and_the_margin_of_its_end() {
    assert_eq!(MARGIN, Duration::from_secs(30 * 60));
    // A long run of two hours is not the kit's two hours: it asks for what it is to take.
    assert_eq!(
        lasting(Duration::from_secs(2 * 3600)),
        Duration::from_secs(2 * 3600 + 30 * 60)
    );
    assert_eq!(lasting(Duration::ZERO), MARGIN);
}

#[test]
fn a_run_with_no_real_claude_code_has_no_login_and_starts_none() {
    assert!(throwaway(false, Duration::from_secs(60)).is_none());
}

#[test]
fn what_the_kit_finds_out_at_the_end_is_kept_for_the_report_and_never_fails_the_rig() {
    let config = Throwaway::over(Says::once(vec![Some(BANNER)]));
    assert_eq!(config.findings(), None);
    assert!(
        config.finish().is_ok(),
        "the run's report is not lost to it"
    );
    let said = config.findings().unwrap();
    assert_eq!(
        said,
        "~/.claude/settings.json is not as it was; \
         ~/.claude/history.jsonl gained 2 prompt(s) typed in a folder of the run; \
         the run's Claude Code wrote a different login in the folder's .credentials.json"
    );
    assert!(!said.contains('!'), "{said}");
}

#[test]
fn a_run_that_left_the_owner_be_has_nothing_to_report_and_what_was_found_stays_found() {
    let config = Throwaway::over(Says::once(vec![None, Some("a banner with no lines"), None]));
    assert!(config.finish().is_ok());
    assert_eq!(config.findings(), None);
    // A rig started again on the same home is asked again; a verdict does not take back another.
    assert!(config.finish().is_ok());
    assert_eq!(config.findings().as_deref(), Some("a banner with no lines"));
    assert!(config.finish().is_ok());
    assert_eq!(config.findings().as_deref(), Some("a banner with no lines"));
}

#[test]
fn the_folder_and_the_records_are_the_kits() {
    let config = Throwaway::over(Says::once(Vec::new()));
    assert_eq!(config.folder().unwrap(), PathBuf::from("/tmp/the-folder"));
    assert_eq!(config.described(), "the test");
    config.keep_records(Path::new("/tmp/into")).unwrap();
    config.keep_records(Path::new("/tmp/into")).unwrap();
    let kept = format!("{config:?}");
    assert!(kept.contains("kept: 2"), "{kept}");
}

/// A machine whose Claude Code windows run on `login`.
fn machine_on(login: Option<Arc<Throwaway>>) -> Machine {
    Machine {
        home: PathBuf::from("/home/u"),
        installs: Vec::new(),
        node: None,
        login,
    }
}

/// What the floor of a run says of the login, and the check the report gets.
fn reported(machine: &Machine) -> (Option<Check>, String) {
    let mut said = Vec::new();
    let check = {
        let mut floor = Floor::new(&mut said);
        left_as_it_was(machine, &mut floor).unwrap()
    };
    (check, String::from_utf8(said).unwrap())
}

#[test]
fn a_report_has_a_check_that_the_owners_claude_was_left_as_it_was_for_a_run_that_had_a_throwaway() {
    let name = "the owner's Claude setup and the machine's login were left as they were";
    // No throwaway, no such thing to leave.
    assert_eq!(reported(&machine_on(None)), (None, String::new()));
    // One that left everything be.
    let quiet = Arc::new(Throwaway::over(Says::once(vec![None])));
    quiet.finish().unwrap();
    let (check, said) = reported(&machine_on(Some(quiet)));
    assert_eq!(
        check.map(|check| (check.name, check.ok)),
        Some((name.to_owned(), true))
    );
    assert_eq!(said, "");
    // One that did not: the check fails, and the floor says what.
    let touched = Arc::new(Throwaway::over(Says::once(vec![Some(BANNER)])));
    touched.finish().unwrap();
    let (check, said) = reported(&machine_on(Some(touched)));
    assert_eq!(
        check.map(|check| (check.name, check.ok)),
        Some((name.to_owned(), false))
    );
    assert!(
        said.contains("the run left the owner's Claude setup or login changed: ~/.claude/settings.json is not as it was; "),
        "{said}"
    );
}
