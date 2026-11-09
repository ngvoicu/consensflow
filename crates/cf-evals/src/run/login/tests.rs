//! The throwaway Claude config folder of an eval: when a run has one, and what it keeps for the
//! report instead of failing the rig.

use std::sync::atomic::{AtomicUsize, Ordering};

use cf_e2e::Error;

use cf_e2e::live::isolation::Isolation;
use cf_e2e::live::Installed;
use cf_e2e::rig::HarnessHome;

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

fn seated(chief: &str, model: &str, harnesses: &[&str], staff_model: &str) -> Seated {
    let models: Vec<(&str, &str)> = harnesses.iter().map(|name| (*name, staff_model)).collect();
    Seated::of(chief, model, &staff_on(harnesses, &models), HOUR)
}

const HOUR: Duration = Duration::from_secs(3600);

#[test]
fn a_run_has_a_real_claude_code_unless_every_one_of_its_windows_is_the_stand_in_agent() {
    let on = |chief: &str, model: &str, harnesses: &[&str], staff_model: &str| {
        seated(chief, model, harnesses, staff_model).real(Harness::Claude)
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

/// Every harness of a run is real or the stand-in by the models of its windows, as Claude Code
/// is (T-785): the kit's homes are made for the real ones, and Pi's holds the logins of its
/// windows' models, chief's and staff's, each once.
#[test]
fn every_harness_of_a_run_is_real_by_the_models_of_its_windows_and_pis_are_each_named_once() {
    let staff = staff_on(
        &["pi", "devin", "codex"],
        &[("pi", "openai/gpt-6-luna"), ("codex", "fake")],
    );
    let run = Seated::of("pi", "openrouter/deepseek/v4", &staff, HOUR);
    assert_eq!(run.planned, HOUR);
    assert!(run.real(Harness::Pi) && run.real(Harness::Devin));
    assert!(!run.real(Harness::Codex), "its one window is the stand-in");
    assert!(!run.real(Harness::Claude) && !run.real(Harness::Muse));
    assert_eq!(
        run.models(Harness::Pi),
        ["openrouter/deepseek/v4", "openai/gpt-6-luna"]
    );
    let again = Seated::of("pi", "openai/gpt-6-luna", &staff, HOUR);
    assert_eq!(again.models(Harness::Pi), ["openai/gpt-6-luna"]);
    // The dry runs: nothing is real, so no home is made for any harness.
    let dry = seated("claude", "fake-chief", &["claude", "devin"], "fake");
    assert!(Harness::ALL.into_iter().all(|harness| !dry.real(harness)));
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
    assert!(throwaway(&seated("codex", "gpt", &["claude"], "fake")).is_none());
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

/// What the report keeps of the login: its check, and the lines of its log (the floor's), one to a
/// line. A line the floor only printed is not in the report (T-785: the owner's hashes were not).
fn reported(machine: &Machine) -> (Option<Check>, String) {
    let mut said = Vec::new();
    let mut floor = Floor::new(&mut said);
    let check = left_as_it_was(machine, &mut floor).unwrap();
    let kept: Vec<String> = floor
        .log()
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["line"].as_str().unwrap().to_owned())
        .collect();
    (check, kept.join("\n"))
}

#[test]
fn a_report_has_a_check_that_the_owners_claude_was_left_as_it_was_for_a_run_that_had_a_throwaway() {
    let name = "the owner's setups and logins of the run's harnesses were left as they were";
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
        said.contains("the run left the owner's setup or login of a harness changed: ~/.claude/settings.json is not as it was; "),
        "{said}"
    );
}

/// An eval's Codex, Pi and Devin windows run for hours on the kit's homes (T-785). A file of the
/// owner's that changes in the run is not the rig's error at its end, which would cost the run its
/// report: the report's check fails, the floor names the file, and the owner's hashes before and
/// after are on the floor (kept in the report's log) whether anything changed or not.
#[test]
fn a_change_to_the_owners_login_in_a_run_on_the_kits_home_fails_the_report_and_not_the_rig() {
    let owner = tempfile::tempdir().unwrap();
    let codex = owner.path().join(".codex");
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(codex.join("auth.json"), r#"{"tokens":"the-owners"}"#).unwrap();
    let isolation = Arc::new(Isolation::codex(owner.path(), &codex).keeping_what_it_finds());
    let machine = Machine {
        installs: vec![Installed::at(Harness::Codex, "/bin/codex", &codex)
            .with_isolation(Arc::clone(&isolation))],
        ..machine_on(None)
    };
    // The rig made the home and ended on it: the owner's login as it was.
    let home = HarnessHome::vars(&*isolation).unwrap();
    assert_eq!(home[0].0, "CODEX_HOME");
    HarnessHome::finish(&*isolation).unwrap();
    let name = "the owner's setups and logins of the run's harnesses were left as they were";
    // The kit says a file below the home as `~/` and the path as the platform spells it.
    let auth = format!("~/{}", Path::new(".codex").join("auth.json").display());
    let (check, said) = reported(&machine);
    assert_eq!(
        check.map(|check| (check.name, check.ok)),
        Some((name.to_owned(), true))
    );
    assert!(
        said.contains(&format!(
            "the owner's Codex setup, before the run and after it: {auth} "
        )),
        "{said}"
    );
    // The run's Codex refreshed the owner's login: the rig ends as it did, the report fails.
    std::fs::write(codex.join("auth.json"), r#"{"tokens":"refreshed"}"#).unwrap();
    assert!(HarnessHome::finish(&*isolation).is_ok());
    let (check, said) = reported(&machine);
    assert_eq!(check.map(|check| check.ok), Some(false));
    assert!(
        said.contains(&format!(
            "the run left the owner's setup or login of a harness changed: {auth} is not as it was"
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!("the owner's Codex setup before the run: {auth} ")),
        "{said}"
    );
    assert!(
        !said.contains("the-owners") && !said.contains("refreshed\""),
        "{said}"
    );
}

#[test]
fn an_evals_config_keeps_its_findings_so_the_guard_of_the_keychain_leaves_the_keychain_to_it() {
    // The scenario evals end the rig with `?`: a guard that failed it for an item another process
    // made in the machine's Keychain would cost them the report (T-1044, T-1102). The guard asks the
    // config; the one that keeps its findings for the report is the one it leaves it to.
    let config = Throwaway::over(Says::once(vec![None]));
    assert!(config.keeps_its_findings());
}
