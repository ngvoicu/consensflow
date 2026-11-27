//! A pause's stop of a Pi window, read by who says that it is at work (the brain's
//! `architecture/pi-turn-state.md`, §7): the two field pauses of 2026-11-26 (an Escape while
//! the model wrote a tool call's arguments, and while it wrote its final text), a compaction at
//! rest, a compaction before a prompt, and a run that does not stop. The rig has no Pi stand-in,
//! so the real Pi reading of what Pi 1.6.0's and 1.6.1's own loops wrote (`cf-harness`'s
//! `tests/fixtures/pi-turn-state`, recorded by `tests/pi-loop/drive.mjs`) is looked at by one
//! reader for the whole case, as the daemon looks, and its look (`cf_harness::pi::look_of`) is
//! given to the dispatcher's fake window at every pass: with the extension's word, and with no
//! word, each in both forms (plain, and with a person's extension's own entries).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cf_base::env::Env;
use cf_engine::testing::Context;
use cf_harness::records::{self, Look, Options, PiSettlement, Reading, Role};
use cf_ledger::MemberEdit;
use cf_proto::agents::Harness;
use serde_json::{json, Value};

const SESSION: &str = "hazy-ridge";
const LAUNCH: &str = "launch-loop";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../cf-harness/tests/fixtures/pi-turn-state")
}

/// The recorded moment `moment` of `case` in Pi `version`: its session's text, and its word with
/// the stage's session.
fn moment(version: &str, case: &str, moment: &str) -> (Option<String>, Value) {
    let dir = fixtures().join(version).join(case).join(moment);
    let text = fs::read_to_string(dir.join("hazy-ridge.jsonl")).ok();
    let mut word: Value = serde_json::from_str(
        &fs::read_to_string(dir.join(format!("{LAUNCH}.state.json"))).unwrap(),
    )
    .unwrap();
    word["sessionId"] = json!(SESSION);
    (text, word)
}

/// The time of a session text's newest entry, in milliseconds.
fn newest(text: &str) -> i64 {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|row| row["timestamp"].as_str().and_then(cf_base::time::parse))
        .max()
        .unwrap()
}

/// A Pi worker's window at work on T-1, its session's folder, and one reader of it.
struct Window {
    context: Context,
    project: i64,
    dir: tempfile::TempDir,
    reader: Box<dyn Look + Send>,
    options: Options,
    /// The clock the reader is told.
    now: i64,
    /// What the fake window's record held once T-1 was delivered (the brief): Pi's assistant
    /// steps are read after it.
    delivered: Vec<cf_harness::records::Item>,
}

impl Window {
    fn new() -> Self {
        let context = Context::new();
        let project = context.with_staff(&["zeus"]);
        let worker = context
            .project(project.id)
            .participants
            .into_iter()
            .find(|participant| participant.handle == "zeus")
            .unwrap();
        context
            .ledger
            .borrow_mut()
            .edit_member(
                project.id,
                "zeus",
                &MemberEdit {
                    harness: "pi".to_owned(),
                    model: None,
                    effort: None,
                    model_name: None,
                    tier: worker.tier.unwrap(),
                },
            )
            .unwrap();
        context.give(project.id, "zeus", "Run the long script");
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(context.task(project.id, 1).task.state, "working");
        let dir = tempfile::tempdir().unwrap();
        let env = Env::from_vars([("PI_CODING_AGENT_SESSION_DIR", dir.path().to_str().unwrap())]);
        let reader =
            records::reader(Harness::Pi, SESSION, &env, &records::zone("UTC").unwrap()).unwrap();
        let options = Options {
            pi_settlement: Some(PiSettlement {
                directory: Some(dir.path().join("settled").to_str().unwrap().to_owned()),
                launch_id: Some(LAUNCH.to_owned()),
            }),
            ..Options::default()
        };
        let delivered = context.adapter.agent("zeus").items.clone();
        Self {
            context,
            project: project.id,
            dir,
            reader,
            options,
            now: 0,
            delivered,
        }
    }

    /// Pi's session as `text` says (none: no file yet) and the extension's word as `word` says
    /// (none: no word), from now on.
    fn shows(&mut self, text: Option<&str>, word: Option<&Value>) {
        let file = self.dir.path().join("hazy-ridge.jsonl");
        match text {
            Some(text) => {
                fs::write(&file, text).unwrap();
                self.now = self.now.max(newest(text) + 1_000);
            }
            None => {
                let _ = fs::remove_file(&file);
            }
        }
        let settled = self.dir.path().join("settled");
        fs::create_dir_all(&settled).unwrap();
        let at = settled.join(format!("{LAUNCH}.state.json"));
        match word {
            Some(word) => fs::write(at, word.to_string()).unwrap(),
            None => {
                let _ = fs::remove_file(at);
            }
        }
    }

    /// One look of the reader, given to the fake window whole, and one pass; the clock then
    /// moves on 3.1 s, the reader's and the engine's. Everything the real reader publishes goes
    /// to the window: its items (after the brief the fake delivered, which the recorded session
    /// holds in its own words), quota, failure and its kind, whether it takes words, and the run
    /// that ended beside the present, likewise.
    fn pass(&mut self) {
        let reading = self.reader.look(&self.options, self.now);
        assert!(
            matches!(&*reading, Reading::Known(_)),
            "Pi's session was not read: {reading:?}"
        );
        let observed = cf_harness::pi::look_of(reading.clone());
        let Reading::Known(record) = &*reading else {
            unreachable!()
        };
        let delivered = self.delivered.clone();
        let bridged = |record: &records::Record| -> Vec<records::Item> {
            let mut items = delivered.clone();
            items.extend(
                record
                    .items
                    .iter()
                    .filter(|item| matches!(item.role, Role::Assistant | Role::Custom | Role::Tool))
                    .cloned(),
            );
            items
        };
        let items = bridged(record);
        let ended = record.lines.ended.as_deref().map(|ended| match ended {
            Reading::Known(ended) => {
                let mut ended = ended.clone();
                ended.items = bridged(&ended);
                Arc::new(Reading::Known(ended))
            }
            other => Arc::new(other.clone()),
        });
        self.context.adapter.with("zeus", |agent| {
            agent.items = items;
            agent.settled = observed.settled;
            agent.takes_words = Some(observed.takes_words);
            agent.over = observed.over;
            agent.failed = observed.failed;
            agent.failure.clone_from(&record.failure);
            agent.failure_kind.clone_from(&record.failure_kind);
            agent.quota.clone_from(&observed.quota);
            agent.said_at_work = record.lines.said_at_work;
            agent.ended = ended;
        });
        self.context.pass().unwrap();
        self.context.advance(3_100);
        self.now += 3_100;
    }

    /// The session growing to `text` a line a look (from what the file held), with the word as
    /// `word` says all along: the reader reads every prefix, as the daemon would.
    fn grows(&mut self, text: &str, word: Option<&Value>) {
        let file = self.dir.path().join("hazy-ridge.jsonl");
        let had = fs::read_to_string(&file).map_or(0, |had| had.lines().count());
        let lines: Vec<&str> = text.lines().collect();
        for upto in (had + 1)..=lines.len() {
            self.shows(Some(&format!("{}\n", lines[..upto].join("\n"))), word);
            self.pass();
        }
    }

    fn passes(&mut self, count: usize) {
        for _ in 0..count {
            self.pass();
        }
    }

    fn escapes(&self) -> usize {
        self.context
            .host
            .inputs()
            .iter()
            .filter(|input| input["bytes"] == json!([27]))
            .count()
    }

    fn told(&self) -> Vec<String> {
        ["human", "chief"]
            .into_iter()
            .flat_map(|who| self.context.inbox(self.context.id(self.project, who)))
            .map(|message| message.body)
            .filter(|body| body.contains("did not stop"))
            .collect()
    }

    fn delivered(&self) -> usize {
        self.context
            .recorder
            .calls("adapter:pi", &["deliver"])
            .len()
    }

    fn pause(&self) {
        self.context.human_pause(self.project, 1);
    }

    /// The chief resumes T-1: its words wait for the window's rest.
    fn resume(&self) -> usize {
        let before = self.delivered();
        self.context
            .resume_task(self.project, 1, "Go on from where you were.");
        before
    }
}

/// The text up to, not with, the last assistant step: the session while the step was written.
fn before_the_step(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let step = lines
        .iter()
        .rposition(|line| {
            serde_json::from_str::<Value>(line).unwrap()["message"]["role"] == "assistant"
        })
        .unwrap();
    format!("{}\n", lines[..step].join("\n"))
}

/// The word as it stood while the run that `word` ended was on.
fn working(word: &Value) -> Value {
    let mut on = word.clone();
    on["state"] = json!("working");
    on["ended"] = json!(0);
    on["ends"] = json!([]);
    on
}

const FORMS: [&str; 4] = ["1.6.0|plain", "1.6.0|extras", "1.6.1|plain", "1.6.1|extras"];

#[test]
fn both_field_pauses_take_one_key_no_note_and_are_paid_with_or_without_the_word() {
    for point in ["escape-text", "escape-call"] {
        for form in FORMS {
            let (version, form) = form.split_once('|').unwrap();
            let (text, word) = moment(version, &format!("{point}-{form}"), "01-interrupted");
            let text = text.unwrap();
            for with_word in [true, false] {
                let what = format!("{point} {version} {form}, word {with_word}");
                let mut window = Window::new();
                // The model writes: at work, by the word or by the file's open user message.
                let writing = before_the_step(&text);
                window.grows(&writing, with_word.then(|| working(&word)).as_ref());
                window.pause();
                window.pass();
                assert_eq!(window.escapes(), 1, "{what}: one key into the run at work");
                // Pi ends the run in the abort; its extension writes the end, then `rest`.
                window.grows(&text, with_word.then_some(&word));
                // With the word at once; without it, after the file's quiet (no key, no note
                // before it).
                window.passes(if with_word { 3 } else { 45 });
                assert_eq!(window.escapes(), 1, "{what}: no key after the first");
                assert_eq!(window.told(), Vec::<String>::new(), "{what}");
                assert_eq!(
                    window.context.task(window.project, 1).task.state,
                    "paused",
                    "{what}"
                );
                // Paid: the Resume's words go in.
                let before = window.resume();
                window.passes(3);
                assert_eq!(
                    window.delivered(),
                    before + 1,
                    "{what}: the Resume delivered"
                );
            }
        }
    }
}

#[test]
fn without_the_word_nothing_is_pressed_or_said_before_the_quiet_has_passed() {
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, _) = moment(version, &format!("escape-call-{form}"), "01-interrupted");
        let text = text.unwrap();
        let mut window = Window::new();
        window.grows(&before_the_step(&text), None);
        window.pause();
        window.pass();
        window.grows(&text, None);
        // Looks until a second short of the quiet after the last entry (the replay's own looks
        // count toward it): neither at rest nor at work. Nothing paid: the Resume waits.
        let before = window.resume();
        while window.now + 3_100 < newest(&text) + 119_000 {
            window.pass();
        }
        assert_eq!(window.escapes(), 1, "{version} {form}");
        assert!(window.told().is_empty(), "{version} {form}");
        assert_eq!(
            window.delivered(),
            before,
            "{version} {form}: nothing goes in before the quiet"
        );
        window.passes(10);
        assert_eq!(
            window.delivered(),
            before + 1,
            "{version} {form}: in once it passed"
        );
    }
}

#[test]
fn a_compaction_at_rest_presses_no_key_and_pays_no_stop_until_pi_rests() {
    // D2 and the second field pause: a person's extension compacts at rest, its handler held.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let case = format!("compaction-at-rest-{form}");
        let (held_text, held) = moment(version, &case, "01-compaction-held");
        let (done_text, done) = moment(version, &case, "02-compacted");
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.shows(held_text.as_deref(), Some(&held));
        window.pass();
        window.pause();
        let before = window.resume();
        // A minute and a half of `busy`: past every round and the minute of a kept window.
        window.passes(30);
        assert_eq!(
            window.escapes(),
            0,
            "{what}: Escape would cancel a compaction that is no turn"
        );
        assert!(window.told().is_empty(), "{what}");
        assert_eq!(
            window.delivered(),
            before,
            "{what}: nothing is sent while Pi is busy"
        );
        window.shows(done_text.as_deref(), Some(&done));
        window.passes(3);
        assert_eq!(window.escapes(), 0, "{what}");
        assert_eq!(
            window.delivered(),
            before + 1,
            "{what}: paid at rest, the Resume delivered"
        );
    }
}

#[test]
fn a_compaction_before_a_prompt_pays_no_stop_and_the_run_after_it_takes_the_keys() {
    // Pi compacts before it begins the run of a prompt in hand: `busy`, not yet a run to pay for.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("stop-{form}"), "02-answered");
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        let mut busy = working(&word);
        busy["state"] = json!("busy");
        window.shows(Some(&before_the_step(&text)), Some(&busy));
        window.pass();
        window.pause();
        window.passes(5);
        assert_eq!(window.escapes(), 0, "{what}");
        window.shows(Some(&before_the_step(&text)), Some(&working(&word)));
        window.pass();
        assert_eq!(window.escapes(), 1, "{what}: the run is at work now");
        window.shows(Some(&text), Some(&word));
        window.passes(3);
        assert_eq!(window.escapes(), 1, "{what}");
        assert!(window.told().is_empty(), "{what}");
        let before = window.resume();
        window.passes(3);
        assert_eq!(window.delivered(), before + 1, "{what}");
    }
}

#[test]
fn a_run_pi_itself_says_is_on_after_every_round_is_kept_and_said_so() {
    let (text, word) = moment("1.6.1", "stop-plain", "02-answered");
    let mut window = Window::new();
    window.shows(
        Some(&before_the_step(&text.unwrap())),
        Some(&working(&word)),
    );
    window.pass();
    window.pause();
    window.passes(8);
    assert_eq!(window.escapes(), 3, "three rounds");
    let told = window.told();
    assert!(!told.is_empty(), "the window that did not stop is said");
    for note in &told {
        assert!(
            note.contains("its harness itself says a turn is on"),
            "{note}"
        );
    }
}

/// The results the chief was given for T-1.
fn results(window: &Window) -> Vec<String> {
    window
        .context
        .inbox(window.context.id(window.project, "chief"))
        .into_iter()
        .filter(|message| message.kind == "result")
        .map(|message| message.body)
        .collect()
}

#[test]
fn a_task_pi_recovered_by_its_own_retry_keeps_it_and_returns_only_the_retrys_words() {
    // Finding 1 and R5: Pi's first attempt failed with a 503 (a refusal for a moment) and words
    // and a call begun; Pi omitted it and retried, and the retry answered. While Pi's word says
    // the run is on, the 503 is no turn's: the task is neither paused nor requeued, and its
    // result is the retry's words alone. The session grows a line a look, everything forwarded.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let case = format!("retry-{form}");
        let (during, at_work) = moment(version, &case, "01-second-request");
        let (text, word) = moment(version, &case, "02-retried");
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&during.unwrap(), Some(&at_work));
        let task = window.context.task(window.project, 1).task;
        assert_eq!(
            task.state, "working",
            "{what}: the 503 in the run is Pi's to handle"
        );
        window.grows(&text.unwrap(), Some(&word));
        window.passes(3);
        let task = window.context.task(window.project, 1).task;
        assert_eq!(task.state, "done", "{what}");
        let results = results(&window);
        assert_eq!(results.len(), 1, "{what}: {results:?}");
        assert!(
            results[0].contains("Retry finished."),
            "{what}: {}",
            results[0]
        );
        assert!(
            !results[0].contains("Running the script now."),
            "{what}: {}",
            results[0]
        );
    }
}

#[test]
fn a_refusal_that_ends_a_run_reaches_the_dispatcher_with_that_end() {
    // R5: the run's last step is a 503 Pi does not retry (its retries spent): that run's end
    // carries the refusal, and the task is held for it, not done.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("final-error-{form}"), "01-failed");
        let text = text.unwrap().replace(
            "400 Invalid request.",
            "503 Service Unavailable: overloaded",
        );
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&before_the_step(&text), Some(&working(&word)));
        window.grows(&text, Some(&word));
        window.passes(3);
        let task = window.context.task(window.project, 1).task;
        assert_ne!(
            task.state, "working",
            "{what}: the refusal reached the dispatcher"
        );
        assert_ne!(task.state, "done", "{what}");
        assert!(results(&window).is_empty(), "{what}");
    }
}

#[test]
fn a_pause_during_a_run_whose_earlier_end_is_unread_presses_the_key_into_that_run() {
    // R1 (finding 3): run 1 ended and its end not yet read; run 2 is on. The stop is for run 2.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(
            version,
            &format!("triggered-after-settle-{form}"),
            "01-two-ends",
        );
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        let mut none = word.clone();
        none["state"] = json!("working");
        none["begun"] = json!(1);
        none["ended"] = json!(0);
        none["ends"] = json!([]);
        let run2 = before_the_step(&text);
        window.grows(&before_the_step(&run2), Some(&none));
        // Run 1 ended, run 2 began, before the next look.
        let mut on = word.clone();
        on["state"] = json!("working");
        on["begun"] = json!(2);
        on["ended"] = json!(1);
        on["ends"] = json!([word["ends"][0]]);
        window.pause();
        window.grows(&run2, Some(&on));
        window.pass();
        assert!(
            window.escapes() >= 1,
            "{what}: a key into run 2: {}",
            window.escapes()
        );
    }
}

#[test]
fn a_rest_whose_leaf_the_file_lacks_takes_no_key_and_is_paid() {
    // R1 (finding 3): Pi's word rests at a leaf the file has not written yet.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("stop-{form}"), "02-answered");
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&before_the_step(&text), Some(&working(&word)));
        window.pause();
        window.shows(Some(&before_the_step(&text)), Some(&word));
        window.passes(3);
        assert_eq!(window.escapes(), 0, "{what}: no key into a window at rest");
        assert!(window.told().is_empty(), "{what}");
        let before = window.resume();
        window.passes(2);
        assert_eq!(
            window.delivered(),
            before + 1,
            "{what}: paid, the Resume delivered"
        );
    }
}

#[test]
fn without_the_word_a_refusal_in_a_retry_holds_the_task_as_on_main() {
    // R5's limit: a window opened before the word (no extension of this release) reads the
    // 503 of a retry as its file shows it, as main does: the task is held for it, and the
    // retry's answer is not its result. Written down, not built.
    let (during, _) = moment("1.6.1", "retry-plain", "01-second-request");
    let (text, _) = moment("1.6.1", "retry-plain", "02-retried");
    let mut window = Window::new();
    window.grows(&during.unwrap(), None);
    let held = window.context.task(window.project, 1).task.state;
    window.grows(&text.unwrap(), None);
    window.passes(45);
    let task = window.context.task(window.project, 1).task;
    eprintln!(
        "without the word: at the 503 {held}; after the retry {}; results {:?}",
        task.state,
        results(&window)
    );
    assert_ne!(held, "working", "the limit as it is: held at the 503");
}

#[test]
fn an_ended_run_beside_a_run_at_work_gives_its_result() {
    // R1 and D3: run 1 answered and ended, a run a person's extension triggered from its end is
    // on, and run 1's end is read only now. Its result is taken from it, as of its own leaf.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(
            version,
            &format!("triggered-after-settle-{form}"),
            "01-two-ends",
        );
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        let run2 = before_the_step(&text);
        let mut none = word.clone();
        none["state"] = json!("working");
        none["begun"] = json!(1);
        none["ended"] = json!(0);
        none["ends"] = json!([]);
        window.grows(&before_the_step(&run2), Some(&none));
        let mut on = word.clone();
        on["state"] = json!("working");
        on["begun"] = json!(2);
        on["ended"] = json!(1);
        on["ends"] = json!([word["ends"][0]]);
        window.grows(&run2, Some(&on));
        window.pass();
        let task = window.context.task(window.project, 1).task;
        assert_eq!(task.state, "done", "{what}");
        let results = results(&window);
        assert_eq!(results.len(), 1, "{what}: {results:?}");
        assert!(
            results[0].contains("First answer done."),
            "{what}: {}",
            results[0]
        );
    }
}

#[test]
fn a_later_omission_of_an_ended_runs_answer_leaves_its_result() {
    // R2 (finding 5): run 1 answered and ended; before its end is read, an entry after it
    // omits that answer. Its result is the answer as of its own leaf.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(
            version,
            &format!("triggered-after-settle-{form}"),
            "01-two-ends",
        );
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        let mut none = word.clone();
        none["state"] = json!("working");
        none["begun"] = json!(1);
        none["ended"] = json!(0);
        none["ends"] = json!([]);
        window.grows(&before_the_step(&before_the_step(&text)), Some(&none));
        let first = text
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .find(|row| row["message"]["role"] == "assistant")
            .unwrap()["id"]
            .clone();
        let omission = json!({ "type": "context_edit", "id": "later-edit", "parentId": word["leaf"], "timestamp": "2026-11-27T10:00:00.000Z", "targetId": first, "replacement": null });
        let mut later = word.clone();
        later["leaf"] = json!("later-edit");
        window.grows(&format!("{text}{omission}\n"), Some(&later));
        window.passes(3);
        let results = results(&window);
        assert_eq!(results.len(), 1, "{what}: {results:?}");
        assert!(
            results[0].contains("First answer done."),
            "{what}: {}",
            results[0]
        );
    }
}

#[test]
fn a_rest_after_a_persons_tree_navigation_returns_the_branchs_answer() {
    // R2 (finding 7): an answer, a later failure, then Pi's `/tree` back to the answer with a
    // label. The window rests on the answer's branch: not failed, and its words are the result.
    for version in ["1.6.0", "1.6.1"] {
        for form in ["plain", "extras"] {
            let (text, word) = moment(version, &format!("tree-{form}"), "01-navigated");
            let what = format!("{version} {form}");
            let mut window = Window::new();
            window.grows(&text.unwrap(), Some(&word));
            window.passes(3);
            let task = window.context.task(window.project, 1).task;
            assert_eq!(task.state, "done", "{what}");
            let results = results(&window);
            assert!(
                results.iter().any(|r| r.contains("First answer")),
                "{what}: {results:?}"
            );
            assert!(
                !results.iter().any(|r| r.contains("Partial answer")),
                "{what}: {results:?}"
            );
        }
    }
}

#[test]
fn on_an_older_pi_a_board_run_whose_start_a_person_holds_past_the_allowance_still_returns_its_answer(
) {
    // R4's limit, recorded on Pi 0.87.0: a person's `before_agent_start` holds the board's own
    // run 3.6 s. Three timer passes and 3 s end it as dropped, at the leaf before the board's
    // message; its answer, later, adds no end. The dropped end reads no answer and moves nothing;
    // the rest after the answer gives the task its result.
    for form in ["plain", "extras"] {
        let (text, word) = moment("0.87.0", &format!("board-start-held-{form}"), "01-answered");
        let text = text.unwrap();
        let drop = word["ends"][0]["leaf"].as_str().unwrap().to_owned();
        let lines: Vec<&str> = text.lines().collect();
        let cut = 1 + lines
            .iter()
            .position(|line| line.contains(&format!("\"id\":\"{drop}\"")))
            .unwrap();
        let before = format!("{}\n", lines[..cut].join("\n"));
        let mut window = Window::new();
        let mut on = word.clone();
        on["state"] = json!("working");
        on["ended"] = json!(0);
        on["ends"] = json!([]);
        on["leaf"] = json!(drop);
        window.grows(&before, Some(&on));
        let mut dropped = word.clone();
        dropped["leaf"] = json!(drop);
        window.shows(Some(&before), Some(&dropped));
        window.pass();
        let at_drop = window.context.task(window.project, 1).task.state;
        window.grows(&text, Some(&word));
        window.passes(3);
        let task = window.context.task(window.project, 1).task;
        assert_eq!(at_drop, "working", "{form}");
        assert_eq!(task.state, "done", "{form}");
        assert_eq!(results(&window), vec!["Board answer".to_owned()], "{form}");
    }
}
