//! Live: whether a Devin window draws its answer when the folders of its home of the run's
//! own are spelled through a Mac's `/var` link (T-724's finding 1, T-1038). It is opt-in
//! (`cargo xtask live linked-folder`): it spends the machine's quota on short tasks of each
//! harness named (Devin, the default), and needs each installed and logged in, so it is
//! ignored unless asked for and is no part of `check` or of CI.
//!
//! The finding (T-724, 2026-10-27, Devin 3000.11.92 and .205): a Devin window run on a home
//! made in the temporary folder, which macOS spells through the link `/var`
//! (`/var/folders/…`, whose real path is `/private/var/folders/…`), never showed its answer:
//! its screen sat on "Typing" while its agent ended the turn in two seconds and wrote the
//! answer to its store. The same home spelled `/private/var/folders/…` passed. A window's
//! own folder can't be what it saw: its program is given that folder as its working
//! folder, which the system resolves (no `PWD` reaches it, T-1036). What it does see are
//! the variables it is given, and Devin's home of the run's own is four of them, its XDG
//! folders ([`cf_e2e::live::isolation`]).
//!
//! So this is that A/B, with the window's folder real (below the person's home) and
//! nothing else changed between the legs: the harness's home of the run's own is made in
//! the temporary folder spelled through the link (**A**), then as the system names it
//! (**B**). Each leg gives the worker a task whose answer is a word its brief does not hold,
//! and holds when the folders the windows were given are spelled as the leg says, the task
//! is done with the word as its result at the chief, and the worker's screen drew the word.
//! A that fails where B holds is the finding, reproduced.
//!
//! ```text
//! cargo xtask live linked-folder                   Devin, A then B, once
//! cargo xtask live linked-folder --rounds 3        three rounds of A then B
//! cargo xtask live linked-folder codex pi          those named (their homes are their own folders)
//! ```
//!
//! A Mac's only: elsewhere the temporary folder is not reached through a link.

#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cf_e2e::live::members::{
    delivered_result, find, is_over, member, stand_in_chief, Account, Members, Plan,
};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, Harness, Log};
use cf_e2e::{files, Error, Result};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the worker is asked: an answer its brief does not spell out.
const BRIEF: &str = "Reply with exactly one line: the words LINKED and FOLDER in capitals, joined by a hyphen, and nothing else.";

/// The answer, which the brief does not hold.
const ANSWER: &str = "LINKED-FOLDER";

/// How long the worker has for its task, and its result to reach the chief.
const WORK: Duration = Duration::from_secs(240);
const DELIVERS: Duration = Duration::from_secs(60);

/// How long the screen has to draw the answer once the task is over: T-724's screens sat on
/// "Typing" for good.
const DRAWS: Duration = Duration::from_secs(30);

/// How often the task and the screen are looked at: the window of a free worker closes
/// once its result is taken.
const LOOK: Duration = Duration::from_millis(200);

/// The harnesses run where none is named.
const DEFAULT: [Harness; 1] = [Harness::Devin];

/// The variables that name a folder of a harness's home of the run's own.
const HOME_VARIABLES: [&str; 6] = [
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
    "CODEX_HOME",
    "PI_CODING_AGENT_DIR",
];

/// A leg of the A/B: the temporary folder the homes are made in, as it is spelled.
struct Leg {
    name: &'static str,
    folder: PathBuf,
}

/// The two legs: the Mac's temporary folder through its link `/var` (A), and as the system
/// names it (B). Both are made from the folder's real path, so a `TMPDIR` spelled either way
/// gives the same two; one outside `/private/var` is no Mac's temporary folder, and refused.
fn legs() -> Result<[Leg; 2]> {
    let real = files::real_path(&std::env::temp_dir());
    let linked = real
        .strip_prefix("/private")
        .ok()
        .filter(|below| below.starts_with("var"))
        .map(|below| Path::new("/").join(below))
        .ok_or_else(|| {
            Error::Daemon(format!(
                "the temporary folder {} is not below /private/var, so no link reaches it: \
                 run with the Mac's own TMPDIR",
                real.display()
            ))
        })?;
    if files::real_path(&linked) != real {
        return Err(Error::Daemon(format!(
            "{} is not {} through the link",
            linked.display(),
            real.display()
        )));
    }
    Ok([
        Leg {
            name: "A, through the link /var",
            folder: linked,
        },
        Leg {
            name: "B, as /private/var",
            folder: real,
        },
    ])
}

/// What was seen of the worker's screen since its task was given.
struct Watch {
    given: Instant,
    /// How long after the task was given the screen first held the answer.
    drawn: Option<Duration>,
    /// The end of the last screen that was not blank.
    screen: String,
}

impl Watch {
    /// The worker's screen looked at once more, while it has a window.
    fn look(&mut self, members: &Members, agent: &str) -> Result {
        if let Some(pane) = members.lane(agent)?["pane"]["id"].as_str() {
            let now = members.screen(pane);
            if self.drawn.is_none() && now.contains(ANSWER) {
                self.drawn = Some(self.given.elapsed());
            }
            if !now.trim().is_empty() {
                self.screen = members.tail(pane);
            }
        }
        Ok(())
    }
}

/// The worker given its task, and what was seen of it, on the leg `leg`.
fn observe(members: &Members, agent: &str, leg: &Leg, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    case.say(format!(
        "the project's folder is {}, as the system names it; the run's homes are made in {}",
        members.folder().display(),
        leg.folder.display()
    ));
    let given: Vec<String> = HOME_VARIABLES
        .iter()
        .filter_map(|name| {
            members
                .rig()
                .var(name)
                .map(|value| format!("{name}={value}"))
        })
        .collect();
    case.check(
        &format!(
            "every folder of the run's home the windows were given is spelled {}",
            leg.name
        ),
        !given.is_empty()
            && HOME_VARIABLES.iter().all(|name| {
                members
                    .rig()
                    .var(name)
                    .is_none_or(|value| Path::new(value).starts_with(&leg.folder))
            }),
        given.join(" "),
    );
    let number = members.give(agent, BRIEF)?;
    let mut watch = Watch {
        given: Instant::now(),
        drawn: None,
        screen: String::new(),
    };
    let over = find(log, &format!("T-{number} to be over"), WORK, LOOK, || {
        watch.look(members, agent)?;
        let thread = members.task(number)?;
        Ok(is_over(&thread).then(|| live::text(&thread["state"])))
    })?;
    let state = over.unwrap_or_else(|| "not over".to_owned());
    if watch.drawn.is_none() {
        find(log, "the screen to draw the answer", DRAWS, LOOK, || {
            watch.look(members, agent)?;
            Ok(watch.drawn.map(|_| ()))
        })?;
    }
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
        watch.drawn.is_some(),
        match watch.drawn {
            Some(after) => format!("{} after the task was given", live::secs(after)),
            None => format!("its screen last showed: {}", watch.screen),
        },
    );
    Ok(case)
}

/// The run of one harness on one leg.
fn run(harness: Harness, leg: &Leg, log: &Log) -> Result<Account> {
    let plan = Plan::new("linked-folder", stand_in_chief())
        .with(member(harness))
        .homes_made_in(&leg.folder);
    let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    members.add(&agent, &[])?;
    let seen = observe(&members, &agent, leg, log);
    let closed = members.close();
    let mut case = seen?;
    case.closed(closed);
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on short tasks of each harness named; cargo xtask live linked-folder"]
fn a_window_whose_home_is_spelled_through_the_var_link_draws_its_answer_as_one_spelled_as_private_var_does(
) -> Result {
    let log = Log::begin("live linked folder")?;
    let mut harnesses: Vec<Harness> = live::list("harness")
        .iter()
        .filter_map(|word| Harness::from_name(word))
        .collect();
    if harnesses.is_empty() {
        harnesses.extend(DEFAULT);
    }
    let rounds = match live::option("rounds") {
        Some(rounds) => rounds
            .parse::<u32>()
            .ok()
            .filter(|rounds| *rounds > 0)
            .ok_or_else(|| {
                Error::Daemon(format!("--rounds is a count of one or more: {rounds}"))
            })?,
        None => 1,
    };
    let legs = legs()?;
    let mut checks = Vec::new();
    for round in 1..=rounds {
        for &harness in &harnesses {
            for leg in &legs {
                let scoped = log.scoped(format!("{} {round} {}", harness.name(), &leg.name[..1]));
                let version = live::version_of(harness);
                let case = run(harness, leg, &scoped).unwrap_or_else(|failed| {
                    let mut case = Account::new();
                    case.fail(&failed);
                    case
                });
                log.block(
                    &format!(
                        "== {} {version}, round {round}, {}: {}",
                        harness.name(),
                        leg.name,
                        if case.held() { "ok" } else { "FAILED" }
                    ),
                    &case.text(),
                );
                checks.push(Check::new(
                    harness.name(),
                    &format!("round {round}, {}: the screen drew the answer", leg.name),
                    case.held(),
                    version,
                ));
            }
        }
    }
    report::conclude(&log, "what each leg came to", &checks)
}
