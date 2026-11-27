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
        Self {
            context,
            project: project.id,
            dir,
            reader,
            options,
            now: 0,
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

    /// One look of the reader, given to the fake window, and one pass; the clock then moves
    /// on 3.1 s, the reader's and the engine's.
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
        self.context.adapter.with("zeus", |agent| {
            agent.items.clear();
            agent.items.extend(
                record
                    .items
                    .iter()
                    .filter(|item| item.role == Role::Assistant)
                    .cloned(),
            );
            agent.settled = observed.settled;
            agent.over = observed.over;
            agent.failed = observed.failed;
            agent.said_at_work = record.lines.said_at_work;
        });
        self.context.pass().unwrap();
        self.context.advance(3_100);
        self.now += 3_100;
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
                window.shows(Some(&writing), with_word.then(|| working(&word)).as_ref());
                window.pass();
                window.pause();
                window.pass();
                assert_eq!(window.escapes(), 1, "{what}: one key into the run at work");
                // Pi ends the run in the abort; its extension writes the end, then `rest`.
                window.shows(Some(&text), with_word.then_some(&word));
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
        window.shows(Some(&before_the_step(&text)), None);
        window.pass();
        window.pause();
        window.pass();
        window.shows(Some(&text), None);
        // 110 s of looks: neither at rest nor at work. Nothing paid: the Resume waits.
        let before = window.resume();
        window.passes(35);
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
