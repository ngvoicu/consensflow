//! The long run, look by look. The owner types the ask into the chief's terminal,
//! and from then on, every few seconds: the watch takes a look and holds it to the
//! invariants, recording each break as it is found; the timeline says what changed;
//! the scripted owner does its next step (an answer, a follow-up, a nudge, an Enter
//! in the chief's dialog), as the evals' owner does; and the disruption that is due
//! is played once its moment comes, the watch looking all the while it waits
//! ([`Watching`]). A disruption whose effect is not seen is a break of its own, with
//! what shows it. The run is over when nothing has moved for the quiet time with the
//! follow-ups said (the project is done), or when the budget is spent. The run waits
//! out the time a job's end may be told after its result, the app is then closed, and
//! the project as the app left it is held to the invariants once more, with whatever
//! of the run's processes the close left (said, and left running), and to the end
//! within the budget; and the report is kept. A ledger the app left that cannot be read
//! fails the run: nothing is held on an earlier copy of it. Everything the run says
//! and keeps, and what stops it, leaves it through its edge ([`Edge`]), the run's own
//! names shown: its console says only the run's own words and identifiers (the
//! harnesses it found, each break's and each unresolved's key as it is found, the
//! summary, where the report is), and the report keeps the rest.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::rig::Project;

use super::disruptions::{Choice, Schedule};
use super::edge::Edge;
use super::invariants::{self, end, Break, Keeper, Settings, Unjudged, EFFECT, END};
use super::ledger::Ledger;
use super::look::{Look, Reading, Windows};
use super::mask::cut;
use super::options::Options;
use super::play::{self, Outcome, Stagehand, Watch, With};
use super::record::Record;
use super::report::{file_name, ChiefKept, Kept, TIME_UP};
use super::setup::{self, Cast};
use super::stage::{self, Stage};
use super::stalls::held::{Prober, System};
use super::stalls::Stalls;
use super::timeline::Timeline;
use crate::measure::{chief_open_question, on_copy};
use crate::plan::terminal_answer;
use crate::run::card::{listed, signature_of, tasks_of};
use crate::run::chief::{Chief, IDLE, READY};
use crate::run::floor::Floor;
use crate::run::owner::{Owner, Seen, Step};
use crate::scenarios::{FollowUp, Scenario};
use crate::Result;

/// How many looks in a row may fail (a copy of the ledger caught mid-write, a
/// daemon slow to answer) before the run is taken to be unable to go on.
const FAILED_LOOKS: u32 = 5;

/// The key Enter sends.
const ENTER: u8 = 13;

/// How long a disruption waits for its effect, and how often it looks.
const WITHIN: Duration = Duration::from_secs(240);
const EVERY: Duration = Duration::from_secs(2);

/// How a run ends, as its report says it.
const DONE: &str = "the project is done";
const STOPPED: &str = "it could not go on";

/// What the run keeps as it goes.
struct Going {
    keeper: Keeper,
    timeline: Timeline,
    schedule: Schedule,
    last: Option<Look>,
    ending: String,
    /// When the budget is spent, in milliseconds: the end (9) is held to it.
    deadline: i64,
    /// Why the ledger the app left could not be read, where it could not.
    final_unread: Option<String>,
    /// What the run keeps of a window that stalled, and of every window at an end the
    /// project did not reach.
    stalls: Stalls,
    /// The ledger the app left, to be copied beside the report where the stalls keep
    /// anything.
    ledger: Option<PathBuf>,
    /// What asks the system of a stalled window's processes.
    prober: Box<dyn Prober>,
    /// Where all the run says and keeps leaves it.
    edge: Edge,
}

impl Going {
    /// A look, held to the invariants with `settings`: what changed told, each break
    /// found recorded and said, what could not be judged said, and each window that
    /// stalled or no longer does told. It is the last look then. The names it shows are
    /// the run's from then, shown wherever it says them.
    fn hold(&mut self, floor: &mut Floor, settings: &Settings, look: Look) {
        self.timeline.changes(self.last.as_ref(), &look);
        self.edge.knows(&self.timeline.names);
        let found = invariants::check(&look, settings);
        self.record(floor, look.at, found);
        let open = invariants::unjudged(&look, settings);
        self.fill(floor, &look, &open);
        self.unjudged(floor, look.at, open);
        for (at, said) in self.stalls.look(&look, settings.idle) {
            let _ = floor.note(&said);
            self.timeline.add(at, "stall", said);
        }
        self.stalls.take(&look, self.prober.as_mut());
        self.last = Some(look);
    }

    /// The gaps `look` fills (`open`, what it could not judge): each said by its key and
    /// told in the timeline with what it came to; what it was stays in the timeline and the
    /// report.
    fn fill(&mut self, floor: &mut Floor, look: &Look, open: &[Unjudged]) {
        for filled in self.keeper.fill(look, open) {
            let gap = &filled.open;
            let _ = floor.say(format!("RESOLVED {}: {}", gap.invariant, gap.key));
            self.timeline.add(
                look.at,
                "resolved",
                format!(
                    "{}: a later look read what it needed, and it {}",
                    gap.key,
                    if filled.broke { "broke" } else { "held" }
                ),
            );
        }
    }

    /// What a look at `at` could not judge (`found`): each not said before is said by
    /// its key, and kept in the log and told in the timeline whole, as unresolved, masked
    /// with the run's names.
    fn unjudged(&mut self, floor: &mut Floor, at: i64, found: Vec<Unjudged>) {
        for open in self.keeper.keep_unjudged(at, found, &self.timeline.names) {
            let _ = floor.say(format!("UNRESOLVED {}: {}", open.invariant, open.key));
            let _ = floor.note(format!("UNRESOLVED {}: {}", open.invariant, open.said));
            self.timeline.add(
                at,
                "unresolved",
                format!("{}: {}", open.invariant, open.said),
            );
        }
    }

    /// What came of the disruption `choice` chose, played `at` into the run: played,
    /// its effect seen (and without the field's shape it is for, where its moment
    /// lacked it, which fails the run); failed, its effect not seen, which is a break of
    /// the tenth invariant with what shows it; or missed, and waiting for its next
    /// moment.
    fn came_of(
        &mut self,
        floor: &mut Floor,
        choice: &Choice,
        at: Duration,
        outcome: Outcome,
    ) -> Result {
        let (disruption, how) = (choice.disruption, choice.how);
        let flag = disruption.flag();
        match outcome {
            Outcome::Played(said) => {
                floor.note(&said)?;
                self.schedule
                    .played(disruption, at, format!("{how}: {said}"));
                self.timeline
                    .add(now(), "disruption", format!("{flag}: {said}"));
                if let Some(lacks) = choice.lacks {
                    self.schedule.lacked(disruption, lacks);
                    floor.note(format!("{flag} was played without {lacks}"))?;
                    self.timeline.add(
                        now(),
                        "disruption",
                        format!("{flag} was played without {lacks}"),
                    );
                }
            }
            Outcome::Failed { said, evidence } => {
                floor.note(format!("{flag} had no effect: {said}"))?;
                self.schedule
                    .failed(disruption, at, format!("{how}: {said}"));
                self.timeline
                    .add(now(), "disruption", format!("{flag} failed: {said}"));
                let found = Break::new(
                    EFFECT,
                    format!("{EFFECT}:{flag}"),
                    format!("{flag}: {said}"),
                )
                .with(evidence);
                self.record(floor, now(), vec![found]);
            }
            Outcome::Missed(why) => {
                floor.note(format!("{flag} missed its moment: {why}"))?;
                self.timeline.add(
                    now(),
                    "disruption",
                    format!("{flag} missed its moment ({why}); it waits for the next"),
                );
            }
        }
        Ok(())
    }

    /// The breaks `found` at `at`: each not recorded before is recorded, said by its key,
    /// and kept in the log and told in the timeline whole, masked with the run's names.
    fn record(&mut self, floor: &mut Floor, at: i64, found: Vec<Break>) {
        for kept in self.keeper.keep_at(at, found, &self.timeline.names) {
            let _ = floor.say(format!(
                "BREAK {} ({}): {}",
                kept.invariant,
                kept.name(),
                kept.key
            ));
            let _ = floor.note(format!(
                "BREAK {} ({}): {}",
                kept.invariant,
                kept.name(),
                kept.said
            ));
            self.timeline
                .add(at, "break", format!("{}: {}", kept.invariant, kept.said));
        }
    }
}

/// The run's watch while a disruption waits: a look as often as the run's own loop
/// takes one, held to the invariants as it holds them.
struct Watching<'a, 'o> {
    run: &'a mut Going,
    floor: &'a mut Floor<'o>,
    settings: &'a Settings,
    every: Duration,
    looked: Instant,
}

impl Watch for Watching<'_, '_> {
    fn tick(&mut self, stage: &mut dyn Stagehand) {
        if self.looked.elapsed() < self.every {
            return;
        }
        self.looked = Instant::now();
        // A look that fails in a disruption (the app quitting, a copy caught
        // mid-write) is the next one's to take.
        if let Ok(look) = stage.look() {
            self.run.hold(self.floor, self.settings, look);
        }
    }

    fn last(&self) -> Option<&Look> {
        self.run.last.as_ref()
    }
}

/// `long`: the run, and its report; the status is 0 when it held. What stops it
/// leaves it through its edge, as all it says does, kept in the reports' folder.
pub fn command(words: &[String], out: &mut dyn Write) -> Result<i32> {
    let edge = Edge::default();
    run(words, out, &edge).map_err(|failed| edge.failure(failed, &reports_of(words)))
}

/// The folder a run of `words` keeps its reports in: none where they are no run's
/// (what stops it then is a usage's error, which needs none).
fn reports_of(words: &[String]) -> PathBuf {
    Options::of(words).map_or_else(|_| PathBuf::new(), |options| options.reports)
}

/// The run of `command`, all it says and keeps leaving through `edge`.
fn run(words: &[String], out: &mut dyn Write, edge: &Edge) -> Result<i32> {
    let options = Options::of(words)?;
    // The harness code of the product is asked as a process (the trust of the
    // workspace, a console's text): built before anything is spent.
    cf_e2e::live::ask::prepare()?;
    let cast = Cast::of(&options)?;
    let mut floor = edge.floor(out);
    for left in &cast.left {
        floor.say(left.said())?;
    }
    for found in &cast.found {
        floor.say(format!(
            "{}: {} ({})",
            found.name,
            found.version,
            found.command.display()
        ))?;
    }
    let scenario = options.ask.scenario(options.quiet);
    let opened = setup::open(&options, &cast, &scenario, &mut floor, edge.masker())?;
    let daemon = opened.daemon.clone();
    let login = opened.login;
    let mut stage = Stage::new(opened.rig, opened.config, opened.project, opened.claude);
    if let Some(span) = options.drop_reading {
        stage.watcher.drop_readings(span);
    }
    let started = now();
    let mut run = Going {
        keeper: Keeper::default(),
        timeline: Timeline::new(started),
        schedule: Schedule::new(
            &options.disruptions,
            options.every,
            &options.chief,
            &cast.names,
        ),
        last: None,
        ending: String::new(),
        deadline: started + millis(options.budget),
        final_unread: None,
        stalls: Stalls::default(),
        ledger: None,
        prober: Box::new(System::new({
            let (edge, folder) = (edge.clone(), options.reports.clone());
            let name = file_name(started, &options.chief, &cast.names);
            move |label| {
                edge.sample_file(&folder, &name, label)
                    .map_err(|failed| failed.to_string())
            }
        })),
        edge: edge.clone(),
    };
    for planned in &run.schedule.planned {
        if let super::disruptions::State::Impossible(why) = &planned.state {
            floor.say(format!(
                "{} cannot be played: {why}",
                planned.disruption.flag()
            ))?;
        }
    }
    let conducted = conduct(&options, &cast, &scenario, &mut stage, &mut floor, &mut run);
    let error = conducted.err().map(|failed| failed.to_string());
    if let Some(error) = &error {
        floor.note(format!("the run stopped: {error}"))?;
        if run.ending.is_empty() {
            run.ending = STOPPED.to_owned();
        }
    }
    let home = finish(&mut stage, &mut run, &mut floor, &options.settings);
    let ended = now();
    let error = match (error, &home) {
        (Some(error), _) => Some(error),
        (None, Err(failed)) => Some(format!("the app did not close: {failed}")),
        (None, Ok(_)) => run.final_unread.clone(),
    };
    let name = file_name(started, &options.chief, &cast.names);
    if let Some(file) = &run.ledger {
        run.stalls.ledger = Some(
            edge.keep_ledger(&options.reports, &name, file)
                .map_err(|failed| failed.to_string()),
        );
    }
    let kept = Kept {
        options: &options,
        chief: ChiefKept {
            harness: options.chief.clone(),
            model: cast.chief.model.clone(),
            effort: options.chief_effort().map(str::to_owned),
        },
        found: &cast.found,
        left: &cast.left,
        staff: &cast.staff,
        daemon,
        home: home.ok(),
        started,
        ended,
        schedule: &run.schedule,
        breaks: &run.keeper.breaks,
        unresolved: &run.keeper.unjudged,
        filled: &run.keeper.filled,
        timeline: &run.timeline,
        stalls: &run.stalls,
        ending: run.ending.clone(),
        error,
        // The app has closed: what the run did to the owner's Claude setup, or to the copy of
        // the login, is known, and a run of hours keeps its report either way.
        login: login.and_then(|login| login.findings()),
        log: floor.log(),
    };
    keep_and_say(&kept, edge, &options.reports, &name, &mut floor)
}

/// Keeps the report `kept` in `folder` as `name`, through `edge`, and says on `floor` its
/// summary and where it is, none of its evidence: that stays on the machine the run ran
/// on (one on zeewin, through `cargo xtask windows`, keeps its files there). The
/// status: 0 where the run held.
fn keep_and_say(
    kept: &Kept,
    edge: &Edge,
    folder: &Path,
    name: &str,
    floor: &mut Floor,
) -> Result<i32> {
    let (json, markdown) = edge.keep(folder, name, &kept.value(), &kept.markdown())?;
    for line in kept.say(&json, &markdown) {
        floor.say(line)?;
    }
    Ok(i32::from(!kept.held()))
}

/// The run's end. A last look while the app runs, held to the invariants with
/// `settings`, and more while the newest result is younger than the time its job's end
/// may be told after it (the app still running to tell it); the app closed; and the
/// project as it left it (its ledger once the app had closed, every exit the run saw,
/// and what of the run's processes was left, said and left running) held to them again
/// and to the end within the budget. A ledger the app left that cannot be read is why
/// the run fails, and only what the close left of the run's processes is held then.
/// Where the app did not come back, the last look that was is held, and nothing it
/// told is told again. Where the project did not reach its end, every window of the last
/// look before the close is kept as it was ([`Stalls`]); where that or a stall was kept,
/// the ledger the app left is to be copied beside the report. The disruptions never
/// played are said so. Where the rig's files are kept, or why the app did not close.
fn finish(
    stage: &mut Stage,
    run: &mut Going,
    floor: &mut Floor,
    settings: &Settings,
) -> Result<PathBuf> {
    if let Ok(look) = Stagehand::look(stage) {
        run.hold(floor, settings, look);
    }
    let left = run
        .last
        .as_ref()
        .map_or(0, |look| settle_left(look, settings.settle, now()));
    if left > 0 {
        let _ = floor.note(format!(
            "waiting {} s for what a job's end may still tell of the newest result",
            left / 1000 + 1
        ));
        let until = Instant::now() + Duration::from_millis(u64::try_from(left).unwrap_or(0));
        while Instant::now() < until {
            sleep(EVERY.min(until.saturating_duration_since(Instant::now())));
            if let Ok(look) = Stagehand::look(stage) {
                run.hold(floor, settings, look);
            }
        }
    }
    read_once_more(run, floor, settings, || Stagehand::look(stage));
    run.stalls
        .before_close(run.last.as_ref(), run.prober.as_mut());
    let ledger = stage
        .rig()
        .ok()
        .map(|rig| rig.home().join("consensflow.db"));
    let closed = stage.close();
    let at = now();
    let after = match &closed {
        Ok(closed) => {
            let ledger = on_copy(&closed.home.join("consensflow.db"), Ledger::read);
            // The records read once more, as the ledger is: a row copied after the last look
            // is held to a record read past it.
            let records = ledger
                .as_ref()
                .ok()
                .map(|ledger| {
                    let records = stage.watcher.records_after_close(ledger);
                    run.timeline.add(at, "run", read_again(&records.1));
                    records
                })
                .unwrap_or_default();
            if !closed.left.is_empty() {
                run.timeline.add(
                    at,
                    "run",
                    format!(
                        "{} process{} of the run left once the app closed",
                        closed.left.len(),
                        if closed.left.len() == 1 {
                            " was"
                        } else {
                            "es were"
                        }
                    ),
                );
            }
            let windows = Windows {
                exits: stage.exits().to_vec(),
                strangers: closed.left.clone(),
                unresolved: closed.unresolved.clone(),
                unread: stage.watcher.unread().to_vec(),
                ..Windows::default()
            };
            after_close(
                at,
                ledger,
                windows,
                records,
                (settings, &run.ending, run.deadline),
            )
        }
        Err(_) => match run.last.clone() {
            Some(look) => {
                let found = final_breaks(&look, settings, &run.ending, run.deadline);
                AfterClose {
                    look: Some(look),
                    found,
                    unread: None,
                }
            }
            None => AfterClose::default(),
        },
    };
    if let Some(why) = &after.unread {
        let _ = floor.note(why);
        run.final_unread = Some(why.clone());
    }
    let at = after.look.as_ref().map_or(at, |look| look.at);
    if let Some(look) = &after.look {
        run.timeline.changes(run.last.as_ref(), look);
    }
    run.record(floor, at, after.found);
    if let Some(look) = after.look {
        let settings = final_settings(settings);
        let found = invariants::unjudged(&look, &settings);
        run.fill(floor, &look, &found);
        run.unjudged(floor, look.at, found);
        run.last = Some(look);
    }
    let unended = run.keeper.broke(END);
    if run
        .stalls
        .after_close(unended, run.last.as_ref().map(|look| &look.ledger))
    {
        run.ledger = ledger;
    }
    let why = match run.ending.as_str() {
        TIME_UP => "time ran out before its moment came",
        STOPPED => "the run stopped before its moment came",
        _ => "the project ended before its moment came",
    };
    run.schedule.close(why);
    closed.map(|closed| closed.root)
}

/// One more look before the app closes, taken with `look`, where a gap a reading may fill
/// is open, or the last look leaves one as the run's end will judge it: a record a result
/// needs, read only as far as before the settle time after it (by a reading begun before
/// then, however late its look ended). The time it needs has passed by now, and a reading
/// begun now reads through it; the clock alone is no reading.
fn read_once_more(
    run: &mut Going,
    floor: &mut Floor,
    settings: &Settings,
    look: impl FnOnce() -> Result<Look>,
) {
    let waits = run.keeper.waits_for_a_reading()
        || run.last.as_ref().is_some_and(|last| {
            invariants::unjudged(last, &final_settings(settings))
                .iter()
                .any(|open| open.gap.is_some())
        });
    if waits {
        if let Ok(look) = look() {
            run.hold(floor, settings, look);
        }
    }
}

/// How long, from `now`, the newest result of `look` is still younger than `settle`:
/// the time its job's end may still be told after it, while the app runs to tell it.
/// None where it is older, or there is no result.
fn settle_left(look: &Look, settle: i64, now: i64) -> i64 {
    look.ledger
        .messages
        .iter()
        .filter(|message| message.kind == "result")
        .filter_map(|result| crate::clock::millis(&result.created_at))
        .max()
        .map_or(0, |newest| (newest + settle - now).clamp(0, settle))
}

/// What the run holds once the app has closed.
#[derive(Debug, Default)]
struct AfterClose {
    /// The look at the project as the app left it: none where its ledger could not be
    /// read.
    look: Option<Look>,
    /// What broke in it.
    found: Vec<Break>,
    /// Why the ledger the app left could not be read, where it could not: the run
    /// fails.
    unread: Option<String>,
}

/// What the run holds once the app has closed at `at`, its ledger read as `ledger`, its
/// records as `records` (read once more after the close, or as the last look read them)
/// and its windows as the close left them, held with `settings` and to the end of a
/// run that ended `why` by `deadline`. A ledger that could not be read leaves no look:
/// what the close left of the run's processes, which needs no ledger, is held all the
/// same, and nothing else is, on an earlier copy or on none.
fn after_close(
    at: i64,
    ledger: Result<Ledger>,
    windows: Windows,
    records: Records,
    (settings, why, deadline): (&Settings, &str, i64),
) -> AfterClose {
    match ledger {
        Ok(ledger) => {
            let look = closed_look(at, ledger, windows, records);
            let found = final_breaks(&look, settings, why, deadline);
            AfterClose {
                look: Some(look),
                found,
                unread: None,
            }
        }
        Err(failed) => {
            let processes = closed_look(at, Ledger::default(), windows, Records::default());
            AfterClose {
                look: None,
                found: invariants::check(&processes, &final_settings(settings)),
                unread: Some(format!(
                    "the ledger the app left could not be read: {failed}"
                )),
            }
        }
    }
}

/// The look at the project once the app has closed, at `at`: its ledger as the app
/// left it, the `windows` as the close left them (none open: every exit the run saw,
/// the processes of the run that were left and those it could not judge, and every
/// close whose host's last word was not read whole), and `records`, each with how far it
/// was read.
fn closed_look(at: i64, ledger: Ledger, windows: Windows, (records, readings): Records) -> Look {
    Look {
        at,
        ledger,
        windows: Windows {
            open: Vec::new(),
            activity: Vec::new(),
            ..windows
        },
        records,
        readings,
        host: None,
    }
}

/// The harnesses' records a look holds, by conversation, and how far each was read.
type Records = (BTreeMap<i64, Record>, BTreeMap<i64, Reading>);

/// What the run's timeline says of the records read once more after the close, each
/// reading's by its conversation: how many, and how many of those readings failed.
fn read_again(readings: &BTreeMap<i64, Reading>) -> String {
    let failed = readings
        .values()
        .filter(|reading| reading.failed.is_some())
        .count();
    let plural = |count: usize| if count == 1 { "" } else { "s" };
    format!(
        "the records were read once more after the close: {} conversation{}, {failed} reading{} failed",
        readings.len(),
        plural(readings.len()),
        plural(failed)
    )
}

/// `settings` for a look once the app has closed: a result is judged however young
/// it is, the ledger and the records being all they will be; and a job's end told
/// within `settle` after its result still says the job had ended by then.
fn final_settings(settings: &Settings) -> Settings {
    Settings {
        young: 0,
        ..*settings
    }
}

/// What the run's last look finds: every invariant, and the end within the budget
/// (`deadline`), the run having ended `why`, with the [`final_settings`].
fn final_breaks(look: &Look, settings: &Settings, why: &str, deadline: i64) -> Vec<Break> {
    let mut found = invariants::check(look, &final_settings(settings));
    found.extend(end(look, why, deadline));
    found
}

/// The run from the ask to its end.
fn conduct(
    options: &Options,
    cast: &Cast,
    scenario: &Scenario,
    stage: &mut Stage,
    floor: &mut Floor,
    run: &mut Going,
) -> Result {
    {
        let rig = stage.rig()?;
        let chief = Chief::new(rig, Project::new(rig, stage.project));
        chief.waits_for(floor, "the chief to be ready", READY, || {
            Ok(chief.state()?.as_deref() == Some(IDLE))
        })?;
        chief.settles(&chief.pane()?);
        floor.note(format!("chief ({}) ready; typing the ask", options.chief))?;
        chief.says(floor, &scenario.prompt)?;
    }
    run.timeline.add(now(), "owner", "the owner typed the ask");
    let with = With {
        hold: (options.every / 4).clamp(Duration::from_secs(15), Duration::from_secs(60)),
        switch_to: options
            .switch_target(&cast.names)
            .map(|harness| format!("long-{harness}-1")),
        within: WITHIN,
        every: EVERY,
    };
    let begun = Instant::now();
    run.deadline = now() + millis(options.budget);
    let mut owner = Owner::new(scenario);
    let (mut signature, mut changed, mut failed) = (String::new(), Instant::now(), 0);
    loop {
        sleep(options.look);
        let looked = stage
            .board()
            .and_then(|board| Ok((stage.look_at(&board)?, board)));
        let (look, board) = match looked {
            Ok(both) => {
                failed = 0;
                both
            }
            Err(failure) => {
                failed += 1;
                floor.note(format!("a look failed ({failed} in a row): {failure}"))?;
                if failed >= FAILED_LOOKS {
                    return Err(failure);
                }
                continue;
            }
        };
        let moment = stage::moment(&board, &look, begun.elapsed());
        run.hold(floor, &options.settings, look);
        let lane = Chief::lane_of(&board);
        let tasks = tasks_of(&board);
        let state = lane["activity"]["state"].as_str().map(str::to_owned);
        let now_signed = signature_of(&board, &lane, &tasks);
        if now_signed != signature {
            signature = now_signed;
            changed = Instant::now();
            floor.note(format!(
                "chief {}; tasks {}",
                state.as_deref().unwrap_or("undefined"),
                listed(&tasks)
            ))?;
        }
        let seen = Seen {
            elapsed: begun.elapsed(),
            quiet: changed.elapsed(),
            busy: state.as_deref() != Some(IDLE)
                || tasks.iter().any(|task| {
                    matches!(
                        task["state"].as_str(),
                        Some("queued" | "working" | "waiting")
                    )
                }),
            dialog: state.as_deref() == Some("waiting"),
            board_empty: tasks.is_empty(),
        };
        let file = stage.rig()?.home().join("consensflow.db");
        let step = owner.step(scenario, &seen, options.budget, &mut || {
            chief_open_question(&file)
        })?;
        if let Some(ending) = owner_does(stage, scenario, floor, run, step)? {
            run.ending = ending;
            return Ok(());
        }
        if let Some(choice) = run.schedule.next(&moment) {
            let (disruption, how) = (choice.disruption, choice.how);
            floor.note(format!("disruption: {} ({how} moment)", disruption.about()))?;
            run.timeline.add(
                now(),
                "disruption",
                format!("{} begins ({how} moment)", disruption.flag()),
            );
            let outcome = {
                let mut watching = Watching {
                    run: &mut *run,
                    floor: &mut *floor,
                    settings: &options.settings,
                    every: options.look,
                    looked: Instant::now(),
                };
                play::play(stage, &mut watching, disruption, &choice.target, &with)?
            };
            run.came_of(floor, &choice, begun.elapsed(), outcome)?;
            changed = Instant::now();
        }
    }
}

/// The owner's step, done: how the run ended, if the step ends it.
fn owner_does(
    stage: &Stage,
    scenario: &Scenario,
    floor: &mut Floor,
    run: &mut Going,
    step: Step,
) -> Result<Option<String>> {
    let rig = stage.rig()?;
    let chief = Chief::new(rig, Project::new(rig, stage.project));
    let say =
        |chief: &Chief, floor: &mut Floor, run: &mut Going, what: &str, words: &str| -> Result {
            let names = &run.timeline.names;
            floor.note(format!("{what}: {}", cut(words, 120, names)))?;
            let said = format!("{what}: {}", cut(words, 200, names));
            run.timeline.add(now(), "owner", said);
            chief.settles(&chief.pane()?);
            chief.says(floor, words)
        };
    match step {
        Step::Wait | Step::Next(FollowUp::Switch) => {}
        Step::Dialog => {
            let pane = chief.pane()?;
            floor.note("the chief's dialog waits: the owner takes its first option")?;
            run.timeline.add(
                now(),
                "owner",
                "took the first option of the chief's dialog",
            );
            chief.press(&pane, &[ENTER])?;
        }
        Step::Reply(end) => {
            let reply = terminal_answer(scenario, &end.text);
            say(
                &chief,
                floor,
                run,
                "the chief asked; the owner answered",
                &reply,
            )?;
        }
        Step::Nudge(nudge) => say(
            &chief,
            floor,
            run,
            "the chief stopped; the owner nudged it",
            &nudge,
        )?,
        Step::Next(FollowUp::Say(next)) => say(&chief, floor, run, "the owner's follow-up", &next)?,
        Step::Over => {
            floor.note("quiet: the project is done")?;
            return Ok(Some(DONE.to_owned()));
        }
        Step::TimeUp => {
            floor.note("time is up")?;
            return Ok(Some(TIME_UP.to_owned()));
        }
    }
    Ok(None)
}

/// The time now, in milliseconds.
fn now() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// A span in milliseconds.
fn millis(span: Duration) -> i64 {
    i64::try_from(span.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
