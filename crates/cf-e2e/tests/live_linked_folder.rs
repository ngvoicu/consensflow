//! Live: a project whose folder the human reached through a link has its windows opened in
//! the real folder, and each harness's window answers there (T-783). It is opt-in (`cargo
//! xtask live linked-folder`): it spends the machine's quota on one short task of each
//! harness named (Devin and Claude Code, the default), and needs each installed and logged
//! in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The finding (T-724, 2026-10-27, Devin 3000.11.92 and .205): a Devin window whose folder
//! is spelled through a Mac's `/var` link (`/var/folders/…`, whose real path is
//! `/private/var/folders/…`) never shows its answer: its screen sits on "Typing" while its
//! agent ends the turn in two seconds and writes the answer to its store. A project under
//! `/tmp`, itself a link to `/private/tmp`, is reached so too. ConsensFlow now opens every
//! window in the folder's real path, and prepares its launch for it.
//!
//! The run is a real harness as the one member of a project whose chief is the stand-in, the
//! project opened in its folder as a link under `/tmp` names it, a link to the folder the
//! live folders keep under the real home ([`Plan::through_a_link`]). The check, as the chief,
//! gives the worker a task whose answer is a word its brief does not hold, and looks at the
//! task and the worker's screen until the task is over. It holds when every window of the
//! worker opened in the folder's real path, the task is done with the word as its result at
//! the chief, and the screen drew the word.
//!
//! ```text
//! cargo xtask live linked-folder                   Devin, then Claude Code
//! cargo xtask live linked-folder devin             one harness
//! ```
//!
//! A link of a folder is a Unix one here: on Windows ConsensFlow takes a folder as named.

#![cfg(unix)]

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{
    delivered_result, find, is_over, member, stand_in_chief, Account, Members, Plan,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{files, Result};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the worker is asked: an answer its brief does not spell out.
const BRIEF: &str = "Reply with exactly one line: the words LINKED and FOLDER in capitals, joined by a hyphen, and nothing else.";

/// The answer, which the brief does not hold.
const ANSWER: &str = "LINKED-FOLDER";

/// How long the worker has for its task, and its result to reach the chief.
const WORK: Duration = Duration::from_secs(240);
const DELIVERS: Duration = Duration::from_secs(60);

/// How often the task and the screen are looked at: the window of a free worker closes
/// once its result is taken.
const LOOK: Duration = Duration::from_millis(200);

/// The harnesses run where none is named.
const DEFAULT: [Harness; 2] = [Harness::Devin, Harness::Claude];

/// The worker given its task, and what was seen of it, held to the finding.
fn observe(members: &Members, agent: &str, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    let real = files::real_path(members.folder());
    let link = members.link().map(Path::to_path_buf).unwrap_or_default();
    case.say(format!(
        "the project is opened as {}, a link to {}",
        link.display(),
        real.display()
    ));
    let number = members.give(agent, BRIEF)?;
    let mut drawn = false;
    let mut screen = String::new();
    let over = find(log, &format!("T-{number} to be over"), WORK, LOOK, || {
        if let Some(pane) = members.lane(agent)?["pane"]["id"].as_str() {
            let now = members.screen(pane);
            drawn |= now.contains(ANSWER);
            if !now.trim().is_empty() {
                screen = members.tail(pane);
            }
        }
        let thread = members.task(number)?;
        Ok(is_over(&thread).then(|| live::text(&thread["state"])))
    })?;
    let state = over.unwrap_or_else(|| "not over".to_owned());
    let opens = members.windows_opened_for(agent);
    let folders: Vec<String> = opens.iter().map(|open| live::text(&open["cwd"])).collect();
    case.check(
        "every window of the worker opened in the folder's real path",
        !folders.is_empty() && folders.iter().all(|folder| Path::new(folder) == real),
        format!("{folders:?}"),
    );
    case.check("the task is done", state == "done", &state);
    let result = find(log, "the result at the chief", DELIVERS, LOOK, || {
        Ok(delivered_result(&members.inbox(Some(members.chief()))?))
    })?;
    case.check(
        "the result reached the chief with the answer",
        result.as_deref().is_some_and(|body| body.contains(ANSWER)),
        result.map_or("none".to_owned(), |body| live::cut(&body, 80)),
    );
    case.check(
        "the worker's screen drew its answer",
        drawn,
        format!("its screen last showed: {screen}"),
    );
    Ok(case)
}

/// The run of one harness.
fn run(harness: Harness, log: &Log) -> Result<Account> {
    let plan = Plan::new("linked-folder", stand_in_chief())
        .with(member(harness))
        .through_a_link();
    let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    members.add(&agent, &[])?;
    let seen = observe(&members, &agent, log);
    let closed = members.close();
    let mut case = seen?;
    case.closed(closed);
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live linked-folder"]
fn a_project_reached_through_a_link_has_its_windows_opened_in_the_real_folder_and_answering_there(
) -> Result {
    let log = Log::begin("live linked folder")?;
    let mut harnesses: Vec<Harness> = live::list("harness")
        .iter()
        .filter_map(|word| Harness::from_name(word))
        .collect();
    if harnesses.is_empty() {
        harnesses.extend(DEFAULT);
    }
    let mut checks = Vec::new();
    for harness in harnesses {
        let scoped = log.scoped(harness.name());
        let version = live::version_of(harness);
        let case = run(harness, &scoped).unwrap_or_else(|failed| {
            let mut case = Account::new();
            case.fail(&failed);
            case
        });
        log.block(
            &format!(
                "== {} {version}: {}",
                harness.name(),
                if case.held() { "ok" } else { "FAILED" }
            ),
            &case.text(),
        );
        checks.push(Check::new(
            harness.name(),
            "a project reached through a link answers in its real folder",
            case.held(),
            version,
        ));
    }
    report::conclude(&log, "what each worker came to", &checks)
}
