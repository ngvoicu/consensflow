//! Live: a member window has what the same harness has when the human opens it in a
//! terminal in the project's folder (T-540, Gabriel, 2026-10-25: "the testers, reviewers,
//! workers, advisors and so on have the same features as their individual harnesses,
//! when opened in the cli"). It is opt-in (`cargo xtask live parity`): it spends the
//! machine's quota on one short task of each harness named, so it is ignored unless
//! asked for and is no part of `check` or of CI.
//!
//! - **OpenCode's skills.** The project's folder names a skills folder of its own in its
//!   `opencode.json` (`skills.paths`), as a human's project or global configuration may.
//!   ConsensFlow gives a window its role's skills in the configuration OpenCode reads
//!   last, and a list there replaces the files' own: before the fix the worker had the
//!   role's skill and not the folder's, which OpenCode opened in a terminal there has.
//!   The worker is asked to name the skills it can load; its answer must name both.
//!
//! ```text
//! cargo xtask live parity                 OpenCode
//! cargo xtask live parity opencode
//! ```

use std::time::Duration;

use cf_e2e::live::members::{self, find, is_over, last_result, Members, Plan};
use cf_e2e::live::{self, last_lines, text, Harness, Log, Who};
use cf_e2e::{Error, Result};

/// The stand-in for the chief's window, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The skill the folder's own configuration names, and the folder it is in.
const FOLDERS_SKILL: &str = "cf-parity-probe";
const SKILLS_FOLDER: &str = "parity-skills";

/// The skill of the worker's role, which ConsensFlow gives it.
const ROLES_SKILL: &str = "consensflow-worker";

/// What the chief asks of the worker.
const TASK: &str = "List the names of every skill you can load, one name per line, and \
nothing else. Load none of them and run no tool.";

/// How long the worker has to do it, and how often it is looked at.
const WORK: Duration = Duration::from_secs(300);
const LOOK: Duration = Duration::from_secs(2);

/// The workers named (`cargo xtask live parity <harness>`), OpenCode where none is.
fn workers() -> Result<Vec<Harness>> {
    let named = live::list("harness");
    if named.is_empty() {
        return Ok(vec![Harness::OpenCode]);
    }
    named
        .iter()
        .map(|word| match Who::named(word, "worker")? {
            Who::Harness(Harness::OpenCode) => Ok(Harness::OpenCode),
            _ => Err(Error::Daemon(format!(
                "live parity checks OpenCode's skills only, not {word}"
            ))),
        })
        .collect()
}

/// The project: its folder's `opencode.json` names a skills folder of its own, which
/// holds one skill.
fn plan(harness: Harness) -> Plan {
    let skill = format!(
        "---\nname: {FOLDERS_SKILL}\ndescription: A skill the project's own configuration names, \
         for ConsensFlow's live parity check.\n---\nSay that the parity check skill was loaded.\n"
    );
    Plan::new("parity-opencode", members::stand_in_chief())
        .with(members::member(harness))
        .with_file(
            "opencode.json",
            format!(r#"{{"skills":{{"paths":["./{SKILLS_FOLDER}"]}}}}"#),
        )
        .with_file(&format!("{SKILLS_FOLDER}/{FOLDERS_SKILL}/SKILL.md"), skill)
}

/// The worker's answer to the task, once its task is over.
fn answer(members: &mut Members, harness: Harness, log: &Log) -> Result<String> {
    let agent = live::agents::id(harness);
    members.add(&agent, &[])?;
    let number = members.give(&agent, TASK)?;
    log.say(format!("T-{number} is given: name the skills you can load"));
    let thread = find(log, "the worker's task to end", WORK, LOOK, || {
        let thread = members.task(number)?;
        Ok(is_over(&thread).then_some(thread))
    })?
    .ok_or_else(|| {
        Error::Daemon(format!(
            "T-{number} did not end within {} s",
            WORK.as_secs()
        ))
    })?;
    if text(&thread["state"]) != "done" {
        return Err(Error::Daemon(format!(
            "T-{number} is {}",
            text(&thread["state"])
        )));
    }
    last_result(&thread)
        .map(|result| text(&result["body"]))
        .ok_or_else(|| Error::Daemon(format!("T-{number} is done with no result")))
}

/// What the product showed of a worker that did not answer: its lane of the board, what
/// its window recorded and last drew, and what the daemon warned of.
fn failure(members: &Members, harness: Harness, log: &Log) {
    let agent = live::agents::id(harness);
    let rig = members.rig();
    log.block(
        "the worker's lane of the board",
        &live::pretty(members.lane(&agent)),
    );
    log.block(
        "what the worker's window recorded",
        &live::transcript(rig, members.project_id(), 1),
    );
    if let Ok(Some(window)) = members.window_of(&agent) {
        log.block(
            "what the worker's window last drew",
            &last_lines(&rig.screen(window.id())),
        );
    }
    log.block("the daemon's log", &live::warnings(&rig.log()).join("\n"));
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live parity"]
fn an_opencode_worker_has_the_skills_its_folders_own_configuration_names_and_its_roles() -> Result {
    let log = Log::begin("live parity")?;
    for harness in workers()? {
        let log = log.scoped(harness.name());
        let mut members = Members::open(&plan(harness), std::path::Path::new(FAKE_AGENT), &log)?;
        let said = answer(&mut members, harness, &log);
        if said.is_err() {
            failure(&members, harness, &log);
        }
        let closed = members.close();
        let said = said?;
        closed?;
        log.block("the worker's answer", &said);
        let missing: Vec<&str> = [FOLDERS_SKILL, ROLES_SKILL]
            .into_iter()
            .filter(|skill| !said.contains(skill))
            .collect();
        if !missing.is_empty() {
            return Err(Error::Daemon(format!(
                "the {} worker did not name {}: it lacks what the same harness has in a terminal there",
                harness.name(),
                missing.join(" or ")
            )));
        }
        log.say(format!(
            "ok: the worker names {FOLDERS_SKILL} (the folder's) and {ROLES_SKILL} (its role's)"
        ));
    }
    Ok(())
}
