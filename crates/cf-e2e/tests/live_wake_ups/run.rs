//! One chief's run: it opens as the chief of a project of its own, the human
//! turns the wake-up calls on a minute apart, and each claim is made of what the
//! daemon logged and what the harness's own record of the chief's conversation
//! holds, never of what the chief said it did ([`verdict`]). In the order they
//! run:
//!
//! 1. the first wake-up: queued a minute after the calls were turned on, in the
//!    chief's record once, under its header, and answered; the board, the
//!    human's inbox and the log gained nothing of the reply;
//! 2. a quiet stretch of two minutes and a half, in which no second wake-up comes
//!    that nothing moved for;
//! 3. the words edited and a task moved: the next one comes a minute after the
//!    edit, with the words intact, in the record once, and answered;
//! 4. the chief typed to make it work longer than an interval: nothing is queued
//!    while it works, and the wake-up that fell due comes after the turn, once.
//!
//! Words the human has typed in a window and not sent hold a wake-up too, where
//! the harness holds for them; no run here can leave any: the human's own typing
//! reaches a window by the desktop app's input alone, and the pane host a run has
//! counts no key as the human's. That hold is the engine's test.

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::ask::{self, Item};
use cf_e2e::live::members::{
    self, find, Account, LongCommand, Members, Plan, ThisMachine, BENCH_BRIEF, STAND_IN_WORKER,
};
use cf_e2e::live::{text, Log, Who};
use cf_e2e::Result;
use serde_json::{json, Value};

use crate::evidence::{self, WakeUp};
use crate::verdict;
use crate::words::{self, BUSY_DONE, EDITED, FIRST_BY_MS, MARK, PROBE, QUIET_MS};

/// The words of a wake-up the human wrote none of their own for: the owner's.
pub const DEFAULT: &str =
    "Do you still have work to do? If tasks are left on the board or in your plan, carry on.";

/// The id the chief of a real harness has in the roster.
const CHIEF: &str = "wake-chief";

/// How long the chief is waited for to be at rest, to answer and to be in a turn.
const AT_REST: Duration = Duration::from_secs(240);
const ANSWERS: Duration = Duration::from_secs(150);
const WORKS: Duration = Duration::from_secs(90);

/// How often the log and the record are looked at while something is waited for.
const LOOK: Duration = Duration::from_secs(2);

/// How long the chief is given to go to work on what was typed to it before the
/// human presses Enter again, and how many times they do.
const TAKES: Duration = Duration::from_secs(6);
const ENTERS: usize = 3;

/// What a chief's run came to.
pub struct Finished {
    pub case: Account,
    pub summary: String,
}

impl Finished {
    /// A run that could not be made: why.
    pub fn failed(failure: &cf_e2e::Error) -> Self {
        let mut case = Account::new();
        case.fail(failure);
        Self {
            case,
            summary: failure.to_string(),
        }
    }
}

/// The project of `who`'s run: it is the chief, in a folder of its own, and the
/// stand-in worker is in the roster for the task that moves the board.
pub fn plan(who: Who) -> Plan {
    let chief = match who {
        Who::StandIn => members::stand_in_chief(),
        Who::Harness(harness) => members::member_as(harness, CHIEF),
    };
    Plan::new(&format!("wake-ups-{}", who.name()), chief)
        .told(BENCH_BRIEF)
        .with(members::stand_in_worker())
}

/// The kind of harness whose record the chief's conversation is in.
fn kind_of(who: Who) -> &'static str {
    match who {
        Who::StandIn => "claude-code",
        Who::Harness(harness) => harness.kind(),
    }
}

/// Runs the chief `who` on the project `plan` opens.
pub fn run(who: Who, plan: &Plan, stand_in: &Path, log: &Log) -> Result<Finished> {
    let log = log.scoped(who.name());
    let members = Members::open(plan, stand_in, &log)?;
    let chief = Chief {
        members: &members,
        who,
        log: &log,
    };
    let mut case = Account::new();
    let ran = chief.steps(&mut case);
    // What the busy command left running in the chief's window is not left: what
    // descends from the window's own process, and nothing outside it.
    let own = members
        .window_of("chief")
        .ok()
        .flatten()
        .and_then(|window| members.window_process(&window));
    if let Some(own) = own {
        let _ = LongCommand::new(MARK).end_in(&ThisMachine, own);
    }
    let closed = members.close();
    ran?;
    case.closed(closed);
    let summary = format!("{} checks", case.lines().len());
    Ok(Finished { case, summary })
}

/// The chief of the run, and what is read of it.
struct Chief<'a> {
    members: &'a Members,
    who: Who,
    log: &'a Log,
}

impl Chief<'_> {
    fn events(&self) -> Vec<Value> {
        self.members.events()
    }

    fn wakes(&self) -> Vec<WakeUp> {
        evidence::wake_ups(&self.events())
    }

    /// What the harness's own record of the chief's conversation holds now, as the
    /// daemon reads it.
    fn items(&self) -> Vec<Item> {
        let Some(session) = evidence::chief_session(&self.events()) else {
            return Vec::new();
        };
        ask::records(
            kind_of(self.who),
            &session,
            self.members.rig().environment(),
        )
        .ok()
        .flatten()
        .map(|record| record.items)
        .unwrap_or_default()
    }

    /// What the chief's window is doing, as the board says.
    fn state(&self) -> Result<String> {
        Ok(text(&self.members.lane("chief")?["activity"]["state"]))
    }

    /// Waits for the chief to read at rest.
    fn at_rest(&self) -> Result<bool> {
        Ok(
            find(self.log, "the chief to be at rest", AT_REST, LOOK, || {
                Ok((self.state()? == "idle").then_some(()))
            })?
            .is_some(),
        )
    }

    /// The human turns the calls on or sets them, a minute apart (the daemon's
    /// answer must be a success), and the time the log says the setting changed
    /// at: none if it never said so.
    fn sets(&self, words: Option<&str>) -> Result<Option<i64>> {
        let mark = self.events().last().map_or(0, evidence::number);
        let mut body = json!({ "on": true, "every": 1, "unit": "minutes" });
        if let Some(words) = words {
            body["text"] = json!(words);
        }
        let answer = self.members.page("wakeup.set", body)?;
        if answer["ok"] != true {
            return Err(cf_e2e::Error::Daemon(format!("wakeup.set: {answer}")));
        }
        find(
            self.log,
            "the change of the setting to be logged",
            Duration::from_secs(15),
            Duration::from_millis(500),
            || {
                Ok(evidence::last_setting(&self.events())
                    .filter(|(n, _)| *n > mark)
                    .map(|(_, at)| at))
            },
        )
    }

    /// Waits for the log to hold `count` wake-ups: all of them.
    fn waits_for_wake_ups(&self, count: usize, within: Duration) -> Result<Option<Vec<WakeUp>>> {
        find(
            self.log,
            &format!("wake-up {count} to be queued"),
            within,
            LOOK,
            || {
                let wakes = self.wakes();
                Ok((wakes.len() >= count).then_some(wakes))
            },
        )
    }

    /// Waits for the record to hold `words` in `count` user items, the last of
    /// which the chief answered: the record then. (The same words come more than
    /// once in a run: the edited ones are the second wake-up's and the third's.)
    fn waits_for_the_answer(&self, words: &str, count: usize) -> Result<Vec<Item>> {
        let found = find(
            self.log,
            &format!("the record to hold the wake-up ({count} with these words) and the chief's answer to it"),
            ANSWERS,
            LOOK,
            || {
                let items = self.items();
                let given = evidence::given(&items, words);
                let answered = given.len() >= count
                    && given
                        .last()
                        .is_some_and(|at| evidence::answered(&items, *at));
                Ok(answered.then_some(items))
            },
        )?;
        Ok(found.unwrap_or_else(|| self.items()))
    }

    /// The human types `words` into the chief's window, and presses Enter again
    /// while the window keeps them, until the chief is at work on them.
    fn types(&self, words: &str) -> Result<()> {
        let rig = self.members.rig();
        self.log.say(format!(
            "the human types into the chief's window: {}",
            cf_e2e::live::cut(words, 120)
        ));
        rig.tell(self.members.project_id(), words)?;
        for _ in 0..=ENTERS {
            sleep(TAKES);
            if self.state()? != "idle" {
                return Ok(());
            }
            let pane = self.members.lane("chief")?["pane"].clone();
            self.log
                .say("the chief's window has not taken the text: the human presses Enter again");
            rig.host(
                "pane.input",
                json!({ "id": pane["id"], "generation": pane["generation"], "bytes": [13] }),
            )?;
        }
        Ok(())
    }

    /// Everything the board and the human's inbox hold that a reply could have
    /// put there: tasks, and messages for the human.
    fn board_and_inbox(&self) -> Result<(usize, usize)> {
        let board = self.members.board()?;
        let in_lanes: usize = board["lanes"].as_array().map_or(0, |lanes| {
            lanes
                .iter()
                .map(|lane| lane["tasks"].as_array().map_or(0, Vec::len))
                .sum()
        });
        let open = board["open"].as_array().map_or(0, Vec::len);
        Ok((in_lanes + open, self.members.inbox(None)?.len()))
    }

    /// The words the log's note `message` holds, as the chief's inbox has them.
    fn body_of(&self, message: i64) -> Result<Option<String>> {
        Ok(self
            .members
            .inbox(Some("chief"))?
            .into_iter()
            .find(|found| found["id"].as_i64() == Some(message))
            .and_then(|found| found["body"].as_str().map(str::to_owned)))
    }

    /// The four parts of the run, each said in the case as the checks it made.
    fn steps(&self, case: &mut Account) -> Result<()> {
        if !self.at_rest()? {
            case.fail("the chief never read at rest");
            return Ok(());
        }
        if find(
            self.log,
            "the chief's conversation to be known",
            Duration::from_secs(60),
            LOOK,
            || Ok(evidence::chief_session(&self.events())),
        )?
        .is_none()
        {
            case.fail("the daemon never bound the chief's conversation: its record cannot be read");
            return Ok(());
        }
        if !self.first(case)? {
            return Ok(());
        }
        self.quiet(case)?;
        if !self.edited_and_moved(case)? {
            return Ok(());
        }
        self.busy(case)
    }

    /// Part 1.
    fn first(&self, case: &mut Account) -> Result<bool> {
        let off = self.members.page("wakeup.get", json!({}))?;
        case.check(
            "off until turned on",
            off["wakeUp"]["on"] == false && off["wakeUp"]["text"] == DEFAULT,
            format!("{}", off["wakeUp"]),
        );
        let Some(setting_ms) = self.sets(None)? else {
            case.fail("the log never said the calls were turned on");
            return Ok(false);
        };
        let Some(wakes) = self.waits_for_wake_ups(1, Duration::from_millis(FIRST_BY_MS as u64))?
        else {
            case.fail("no wake-up was queued in time");
            return Ok(false);
        };
        let items = self.waits_for_the_answer(DEFAULT, 1)?;
        let first = &wakes[..1];
        let (held, said) = verdict::the_first(setting_ms, first);
        case.check("the first wake-up", held, said);
        let (held, said) = verdict::reached_once(&items, DEFAULT, wakes[0].message);
        case.check("it reached the chief once", held, said);
        let (held, said) = verdict::the_chief_answered(&items, DEFAULT);
        case.check("the chief answered it", held, said);
        let (tasks, inbox) = self.board_and_inbox()?;
        let moved = evidence::moves(&self.events(), wakes[0].n, None, &wakes);
        let (held, said) = verdict::gained_nothing(&moved, tasks, inbox);
        case.check("its reply stayed in the terminal", held, said);
        self.at_rest()?;
        Ok(true)
    }

    /// Part 2.
    fn quiet(&self, case: &mut Account) -> Result<()> {
        self.log.say(format!(
            "watching for {} s: no second wake-up comes of a quiet board",
            QUIET_MS / 1000
        ));
        sleep(Duration::from_millis(QUIET_MS));
        let events = self.events();
        let wakes = evidence::wake_ups(&events);
        let (held, said) = verdict::quiet(&events, &wakes);
        case.check(
            "no second wake-up within two minutes and a half",
            held,
            said,
        );
        Ok(())
    }

    /// Part 3: the words edited, a task moved, and the next wake-up.
    fn edited_and_moved(&self, case: &mut Account) -> Result<bool> {
        let before = self.wakes().len();
        let Some(edit_ms) = self.sets(Some(EDITED))? else {
            case.fail("the log never said the words were edited");
            return Ok(false);
        };
        let added = self.members.page(
            "member.add",
            json!({ "agent": STAND_IN_WORKER, "roles": ["worker"] }),
        )?;
        let tier = added["member"]["tier"].as_str().unwrap_or_default();
        let mut task: Vec<String> = vec!["task".to_owned(), "add".to_owned()];
        task.extend(members::tier_flag(tier));
        task.extend(["--json".to_owned(), PROBE.to_owned()]);
        let given = self.members.as_chief(&task)?;
        self.log.say(format!(
            "the chief gives the worker a task: the board moved ({})",
            given.output().trim()
        ));
        let Some(wakes) =
            self.waits_for_wake_ups(before + 1, Duration::from_millis(FIRST_BY_MS as u64))?
        else {
            case.fail("no wake-up followed the move of a task and the edit of the words");
            return Ok(false);
        };
        let next = &wakes[before];
        let body = self.body_of(next.message)?;
        let (held, said) = verdict::the_next(edit_ms, Some(next), body.as_deref(), EDITED);
        case.check("the next wake-up, a minute after the edit", held, said);
        let items = self.waits_for_the_answer(EDITED, 1)?;
        let (held, said) = verdict::edited_words_arrived(&items, EDITED, next.message);
        case.check("the edited words arrived intact", held, said);
        let (held, said) = verdict::quiet(&self.events(), &wakes);
        case.check("each wake-up so far came after something moved", held, said);
        self.at_rest()?;
        Ok(true)
    }

    /// Part 4: the chief in a turn that outlasts the interval.
    fn busy(&self, case: &mut Account) -> Result<()> {
        let before = self.wakes();
        let mark = self.events().last().map_or(0, evidence::number);
        self.types(&words::busy(self.who))?;
        let Some(from_ms) = find(self.log, "the chief to be at work", WORKS, LOOK, || {
            Ok(evidence::chief_became(&self.events(), "working", mark))
        })?
        else {
            case.fail("the chief never went to work on what was typed to it");
            return Ok(());
        };
        let Some(wakes) = self.waits_for_wake_ups(before.len() + 1, Duration::from_secs(300))?
        else {
            case.fail("no wake-up followed the chief's turn");
            return Ok(());
        };
        let later = &wakes[before.len()];
        let to_ms = evidence::chief_rested_before(&self.events(), later.n).unwrap_or(from_ms);
        let (held, said) = verdict::not_while_busy(&wakes, from_ms, to_ms);
        case.check("nothing went into the chief's turn", held, said);
        // The edited words came once before the turn (the second wake-up): this is the third.
        let items = self.waits_for_the_answer(EDITED, 2)?;
        let (held, said) = verdict::followed_the_turn(&items, EDITED, BUSY_DONE);
        case.check(
            "the wake-up that fell due in the turn came after it",
            held,
            said,
        );
        case.check(
            "and was one",
            wakes.len() == before.len() + 1,
            format!(
                "{} wake-ups queued since the chief was typed to",
                wakes.len() - before.len()
            ),
        );
        Ok(())
    }
}
