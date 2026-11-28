//! A Pi window read by who says that it is at work (the brain's `architecture/pi-turn-state.md`):
//! the two field pauses of 2026-11-26 (an Escape while the model wrote a tool call's arguments,
//! and while it wrote its final text), a compaction at rest, a compaction before a prompt, a run
//! that does not stop, a retry, a refusal, a rest the file is behind, `/tree`, and a window that
//! goes away just after a run ended. The rig has no Pi stand-in,
//! so the real Pi reading of what Pi 1.6.0's and 1.6.1's own loops wrote (`cf-harness`'s
//! `tests/fixtures/pi-turn-state`, recorded by `tests/pi-loop/drive.mjs`) is looked at by one
//! reader for the whole case, as the daemon looks, and its look (`cf_harness::pi::look_of`) is
//! given to the dispatcher's fake window at every pass: with the extension's word, and with no
//! word, each in both forms (plain, and with a person's extension's own entries).

use std::fs;
use std::path::{Path, PathBuf};

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
    /// What the fake window's record held once T-1 was delivered (the brief): it stands for the
    /// recorded session's first user message, which is that brief in its own words.
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
        self.says(word);
    }

    /// The extension's word as `word` says (none: no word), the file as it is.
    fn says(&self, word: Option<&Value>) {
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

    /// One look of the reader, given to the fake window whole: the real reader's record itself
    /// (its quota, its aborted mark, its lines and every other field), with its items standing
    /// after the brief the fake delivered in place of the recorded one, a person's later prompts
    /// kept; and the look the Pi adapter makes of it (settled, whether it takes words, over).
    fn look(&mut self) {
        let reading = self.reader.look(&self.options, self.now);
        assert!(
            matches!(&*reading, Reading::Known(_)),
            "Pi's session was not read: {reading:?}"
        );
        let observed = cf_harness::pi::look_of(reading.clone());
        let Reading::Known(record) = &*reading else {
            unreachable!()
        };
        let mut items = self.delivered.clone();
        let mut brief = true;
        for item in &record.items {
            if item.role == Role::User && std::mem::take(&mut brief) {
                continue;
            }
            items.push(item.clone());
        }
        self.context.adapter.with("zeus", |agent| {
            agent.items = items;
            agent.settled = observed.settled;
            agent.takes_words = Some(observed.takes_words);
            agent.over = observed.over;
            agent.failed = observed.failed;
            agent.failure.clone_from(&record.failure);
            agent.failure_kind.clone_from(&record.failure_kind);
            agent.quota.clone_from(&observed.quota);
            agent.record = Some(record.clone());
        });
    }

    /// One look given to the fake window, and one pass; the clock then moves on 3.1 s, the
    /// reader's and the engine's.
    fn pass(&mut self) {
        self.look();
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
        self.says(word);
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

    fn state(&self) -> String {
        self.context.task(self.project, 1).task.state
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

/// The word as it stood while the last run that `word` counts was on.
fn working(word: &Value) -> Value {
    let mut on = word.clone();
    let begun = word["ended"].as_u64().unwrap().max(1);
    on["state"] = json!("working");
    on["begun"] = json!(begun);
    on["ended"] = json!(begun - 1);
    on["last"] = Value::Null;
    on
}

/// `word` with its state `state`.
fn with_state(word: &Value, state: &str) -> Value {
    let mut changed = word.clone();
    changed["state"] = json!(state);
    changed
}

const FORMS: [&str; 4] = ["1.6.0|plain", "1.6.0|extras", "1.6.1|plain", "1.6.1|extras"];

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
                assert_eq!(window.state(), "paused", "{what}");
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
        let (held_text, done_text) = (held_text.unwrap(), done_text.unwrap());
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&before_the_step(&held_text), Some(&working(&held)));
        window.grows(&held_text, Some(&held));
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
        window.grows(&done_text, Some(&done));
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
        let (text, word) = moment(version, &format!("stop-{form}"), "03-answered");
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        let busy = with_state(&working(&word), "busy");
        let mut before_the_run = busy.clone();
        before_the_run["begun"] = before_the_run["ended"].clone();
        window.grows(&before_the_step(&text), Some(&before_the_run));
        window.pause();
        window.passes(5);
        assert_eq!(window.escapes(), 0, "{what}");
        window.says(Some(&working(&word)));
        window.pass();
        assert_eq!(window.escapes(), 1, "{what}: the run is at work now");
        window.grows(&text, Some(&word));
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
    let (text, word) = moment("1.6.1", "stop-plain", "03-answered");
    let mut window = Window::new();
    window.grows(&before_the_step(&text.unwrap()), Some(&working(&word)));
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

#[test]
fn a_pause_during_a_run_begun_before_any_look_presses_the_key_into_it() {
    // Finding 3: run 1 ended and run 2 began before the engine looked. The key goes by the
    // present alone: `working` with more runs begun than ended.
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
        let mut run1 = working(&word);
        run1["begun"] = json!(1);
        run1["ended"] = json!(0);
        window.grows(&before_the_step(&run2), Some(&run1));
        // Run 1 ended, run 2 began, before the next look.
        let on = working(&word);
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
fn a_task_pi_recovered_by_its_own_retry_keeps_it_and_returns_only_the_retrys_words() {
    // Finding 1: Pi's first attempt failed with a 503 (a refusal for a moment) and words and a
    // call begun; Pi omitted it and retried, and the retry answered. While Pi's word says the run
    // is on, the 503 is no turn's: the task is neither paused nor requeued, and its result is the
    // retry's words alone. The session grows a line a look, everything forwarded.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let case = format!("retry-{form}");
        let (during, at_work) = moment(version, &case, "01-second-request");
        let (text, word) = moment(version, &case, "02-retried");
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&during.unwrap(), Some(&at_work));
        assert_eq!(
            window.state(),
            "working",
            "{what}: the 503 in the run is Pi's to handle"
        );
        window.grows(&text.unwrap(), Some(&word));
        window.passes(3);
        assert_eq!(window.state(), "done", "{what}");
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
fn a_refusal_that_ends_a_run_reaches_the_dispatcher_at_rest() {
    // Findings 1 and 20: the run's last step is a 503 Pi does not retry (its retries spent): at
    // rest the refusal and its failure are the turn's, and the task is held for it, not done.
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
        window.grows(&text, Some(&with_state(&word, "working")));
        assert_eq!(
            window.state(),
            "working",
            "{what}: an end being written is neither"
        );
        window.says(Some(&word));
        window.passes(3);
        let state = window.state();
        assert_ne!(
            state, "working",
            "{what}: the refusal reached the dispatcher"
        );
        assert_ne!(state, "done", "{what}");
        assert!(results(&window).is_empty(), "{what}");
    }
}

#[test]
fn a_rest_whose_leaf_the_file_lacks_takes_no_key_is_paid_and_sends_nothing_until_the_leaf() {
    // Finding 3, both halves: Pi's word rests at a leaf the file has not written yet. No key, the
    // stop paid; the result and the next message wait for the leaf.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("stop-{form}"), "03-answered");
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&before_the_step(&text), Some(&working(&word)));
        window.pause();
        window.says(Some(&word));
        window.passes(3);
        assert_eq!(window.escapes(), 0, "{what}: no key into a window at rest");
        assert!(window.told().is_empty(), "{what}");
        assert_eq!(window.state(), "paused", "{what}: paid");
        let before = window.resume();
        window.passes(3);
        assert_eq!(
            window.delivered(),
            before,
            "{what}: the Resume waits for the leaf"
        );
        window.grows(&text, Some(&word));
        window.passes(2);
        assert_eq!(
            window.delivered(),
            before + 1,
            "{what}: delivered once the file holds it"
        );
    }
}

#[test]
fn without_the_word_a_refusal_in_a_retry_holds_the_task_as_on_main() {
    // The limit: a window with no word (opened before this extension, or on a Pi that sets its
    // run flag late) reads the 503 of a retry as its file shows it, as main does: the task is held
    // for it, and the retry's answer is not its result. Written down, not built.
    let (during, _) = moment("1.6.1", "retry-plain", "01-second-request");
    let (text, _) = moment("1.6.1", "retry-plain", "02-retried");
    let mut window = Window::new();
    window.grows(&during.unwrap(), None);
    let held = window.state();
    window.grows(&text.unwrap(), None);
    window.passes(45);
    eprintln!(
        "without the word: at the 503 {held}; after the retry {}; results {:?}",
        window.state(),
        results(&window)
    );
    assert_ne!(held, "working", "the limit as it is: held at the 503");
}

#[test]
fn a_rest_after_a_persons_tree_navigation_returns_the_branchs_answer() {
    // Finding 7: an answer, a later failure, then Pi's `/tree` back to the answer with a label.
    // The window rests on the answer's branch: not failed, and its words are the result.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("tree-{form}"), "01-navigated");
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&before_the_step(&text), Some(&working(&word)));
        window.grows(&text, Some(&word));
        window.passes(3);
        assert_eq!(window.state(), "done", "{what}");
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

#[test]
fn a_rest_after_tree_navigation_back_to_a_failure_is_that_failure_not_a_later_abort() {
    // Finding 17: a genuine final 400, a later run interrupted, then `/tree` back to the 400:
    // the window rests on the failure, and the task is held for it, not left working.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("tree-abort-{form}"), "01-navigated");
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&before_the_step(&text), Some(&working(&word)));
        window.grows(&text, Some(&word));
        window.passes(3);
        let state = window.state();
        assert_ne!(
            state, "working",
            "{what}: the failure reached the dispatcher"
        );
        assert_ne!(state, "done", "{what}");
        assert!(results(&window).is_empty(), "{what}");
    }
}

/// A process number no process of the system can have (past every 32-bit signed pid, which
/// `cf_process::alive` reads as none on every platform): the window's process, gone.
const GONE_PID: u32 = u32::MAX;

#[test]
fn a_window_that_goes_away_just_after_its_run_ended_gives_its_answer() {
    // Finding 16: Pi settled (the word `working`, as many ends as runs) and the window's process
    // is gone before the next pass could say `rest`. The engine's one look before it retires the
    // window takes the answer.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("stop-{form}"), "02-settling");
        let text = text.unwrap();
        let what = format!("{version} {form}");
        let mut window = Window::new();
        window.grows(&before_the_step(&text), Some(&working(&word)));
        // While Pi's process is there, an end being written is neither.
        let mut alive = word.clone();
        alive["pid"] = json!(std::process::id());
        window.grows(&text, Some(&alive));
        window.passes(2);
        assert_eq!(window.state(), "working", "{what}");
        assert!(results(&window).is_empty(), "{what}");
        // The window goes: its process with it.
        let mut gone = word.clone();
        gone["pid"] = json!(GONE_PID);
        window.says(Some(&gone));
        window.look();
        window.context.exit("zeus");
        let results = results(&window);
        assert_eq!(results.len(), 1, "{what}: {results:?}");
        assert!(results[0].contains("Done."), "{what}: {}", results[0]);
        assert_eq!(window.state(), "done", "{what}");
    }
}

#[test]
fn a_pause_during_a_retrys_back_off_after_a_reload_presses_one_key() {
    // Finding 28: a prompt let go during a reload's shutdown, its first request a 503, and Pi's
    // retry back-off going on after the new instance started. Pi's run flag is on: the word says
    // `working`, and a pause presses one Escape into the back-off (Pi's cancel of the retry: the
    // published loops show no second request after it), and no more.
    for form in FORMS {
        let (version, form) = form.split_once('|').unwrap();
        let (text, word) = moment(version, &format!("reload-backoff-{form}"), "01-backoff");
        let what = format!("{version} {form}");
        assert_eq!(
            (
                word["state"].clone(),
                word["begun"].clone(),
                word["ended"].clone()
            ),
            (json!("working"), json!(1), json!(0)),
            "{what}"
        );
        let mut window = Window::new();
        window.grows(&text.unwrap(), Some(&word));
        assert_eq!(
            window.state(),
            "working",
            "{what}: the 503 in the back-off is Pi's to handle"
        );
        window.pause();
        window.pass();
        assert_eq!(window.escapes(), 1, "{what}: one key into the back-off");
        // Pi ends the run at the Escape: its word rests at that end.
        let mut rested = word.clone();
        rested["state"] = json!("rest");
        rested["ended"] = json!(1);
        rested["last"] = json!({ "leaf": word["leaf"], "aborted": false });
        window.says(Some(&rested));
        window.passes(3);
        assert_eq!(window.escapes(), 1, "{what}: no key after it");
        assert!(window.told().is_empty(), "{what}");
        assert_eq!(window.state(), "paused", "{what}: paid");
    }
}
