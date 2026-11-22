//! The Admin page's verdict is the launch's own check (P2, A2): the same function, the
//! same questions, the same answers kept. Each case here is a world of its own, with a
//! home, a stand-in CLI and the programs it answers.

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_process::{CaptureFailed, Captured, Failed};
use cf_proto::agents::Harness;

use super::{captured, Fixture};
use crate::admin::{HarnessAdmin, Row, Setup, Update, Version};
use crate::contract::{Adapter, Agent, Launch, LaunchId, Prepared};
use crate::seams::{Processes, Time};
use crate::testing::{
    fake_executable, finished, named, Driver, Fakes, ManualTime, Response, ScriptedLatest, EPOCH_MS,
};
use crate::{codex::CodexAdapter, devin::DevinAdapter, muse::MuseAdapter};

const LAUNCH: &str = "11111111-1111-4111-8111-111111111111";

/// The sentence a launch refuses a Codex with no native queue.
fn no_queue(codex: &str) -> String {
    format!(
        "{codex} has no native queue, which ConsensFlow needs to reach its window: update Codex."
    )
}

const FLAGS: &str = "Usage: codex queue --thread <ID> --message <TEXT>\n";
const NO_FLAGS: &str = "Usage: codex queue\n";

/// What a scripted run answers: it ended with `code` and wrote `stdout`.
fn exits(code: i32, stdout: &str) -> Result<String, Failed> {
    if code == 0 {
        return Ok(stdout.to_owned());
    }
    Err(Failed {
        message: "Command failed: x\n".to_owned(),
        code: Some(code),
        killed: false,
        stdout: stdout.to_owned(),
    })
}

/// A run that did not answer: ended for its time, or by a signal.
fn unanswered(killed: bool) -> Result<String, Failed> {
    Err(Failed {
        message: "Command failed: x\n".to_owned(),
        code: None,
        killed,
        stdout: String::new(),
    })
}

/// A world of the launch's and the page's own: one home, one CLI found on its path,
/// the programs scripted, the engine's probes shared by the launch and the page.
struct World {
    _home: tempfile::TempDir,
    bin: PathBuf,
    fakes: Fakes,
    services: crate::seams::Services,
    admin: Rc<HarnessAdmin>,
    harness: Harness,
}

impl World {
    /// A world with `harness`'s CLI installed as `command`.
    fn new(harness: Harness) -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = home.path();
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let path = bin.to_string_lossy().into_owned();
        let env = Env::from_vars([
            ("HOME", root.to_str().unwrap().to_owned()),
            ("USERPROFILE", root.to_str().unwrap().to_owned()),
            ("PATH", path),
            (
                "XDG_DATA_HOME",
                root.join("data").to_str().unwrap().to_owned(),
            ),
            (
                "XDG_CONFIG_HOME",
                root.join("config").to_str().unwrap().to_owned(),
            ),
            (
                "CONSENSFLOW_HOME",
                root.join("consensflow").to_str().unwrap().to_owned(),
            ),
        ]);
        let fakes = Fakes::new(&env);
        let services = fakes.services(&env, root);
        let admin = Rc::new(HarnessAdmin::new(
            env.clone(),
            Rc::clone(&fakes.time) as Rc<dyn Time>,
            Rc::new(ScriptedLatest::default()),
            Rc::new(crate::testing::ScriptedCapture::default()),
            Rc::clone(&services.probes),
            Rc::clone(&fakes.processes) as Rc<dyn Processes>,
        ));
        Self {
            _home: home,
            bin,
            fakes,
            services,
            admin,
            harness,
        }
    }

    /// The CLI, installed.
    fn install(&self, command: &str) -> PathBuf {
        let at = self.bin.join(command);
        // Scripted programs run nothing: on Windows the file is the `.exe` a PATH finds.
        if cfg!(windows) {
            let exe = at.with_extension("exe");
            fs::write(&exe, "").unwrap();
            return exe;
        }
        fake_executable(&at)
    }

    /// The installed `command` is another file's worth: its size and time are not what they were.
    fn rewrite(&self, command: &str, extra: &str) {
        let at = self.install_path(command);
        let mut text = fs::read_to_string(&at).unwrap_or_default();
        text.push_str(extra);
        fs::write(&at, text).unwrap();
    }

    fn install_path(&self, command: &str) -> PathBuf {
        let at = self.bin.join(command);
        if cfg!(windows) {
            at.with_extension("exe")
        } else {
            at
        }
    }

    fn answer(&self, program: &str, answer: Result<String, Failed>) {
        self.fakes.processes.run_answer(program, answer);
    }

    fn always(&self, program: &str, answer: Result<String, Failed>) {
        self.fakes.processes.always_answer(program, answer);
    }

    /// The row of the harness now, as a look with no refresh.
    fn look(&self, refresh: bool) -> Rc<Row> {
        let id = self.harness.as_str();
        finished(Box::pin(self.admin.check(Some(id), refresh)))
            .unwrap()
            .remove(0)
    }

    /// A launch of the harness's worker, as the engine prepares it.
    fn prepare(&self) -> Result<Prepared, String> {
        let id = LaunchId::new(LAUNCH).unwrap();
        let launch = Launch {
            id: &id,
            project: 1,
            handle: "w-1",
            role: "worker",
            directory: "/work",
            resume: None,
            previous_gone: true,
            message: Some("Do it."),
            agent: Some(Agent {
                model: None,
                effort: None,
                thinking: None,
                designer: false,
            }),
            instructions: "# ConsensFlow worker",
            specs: None,
            compaction: None,
            advisor_off: false,
            suggestion_off: false,
            first_message_ms: 1,
            starts_given_up: 0,
        };
        let adapter: Box<dyn Adapter> = match self.harness {
            Harness::Codex => Box::new(CodexAdapter::new(&self.services)),
            Harness::Devin => Box::new(DevinAdapter::new(&self.services)),
            _ => Box::new(MuseAdapter::new(&self.services)),
        };
        finished(adapter.prepare(&launch))
    }

    /// What was run since the last time, by name, in order.
    fn ran(&self) -> Vec<String> {
        self.fakes
            .processes
            .take_ran()
            .iter()
            .map(|(program, _)| named(program))
            .collect()
    }
}

/// One row of the paired table: the CLI's answers, and what both the launch and the page
/// say of it: the sentence of a refusal, or none where the launch gets past its check.
struct Pair {
    what: &'static str,
    /// A Muse's pin, its binary laid down or not.
    pin: Option<(&'static str, bool)>,
    answers: Vec<(&'static str, Result<String, Failed>)>,
    refusal: Option<String>,
    /// Whether the page says the CLI was read (an update mends it): `UpdateRequired`.
    read: bool,
}

fn codex_pairs() -> Vec<Pair> {
    let version = |text: &str| ("codex --version", Ok(text.to_owned()));
    vec![
        Pair {
            what: "both flags",
            pin: None,
            answers: vec![("codex queue --help", Ok(FLAGS.to_owned()))],
            refusal: None,
            read: true,
        },
        Pair {
            what: "no flags, exit 2, a version",
            pin: None,
            answers: vec![
                ("codex queue --help", exits(2, NO_FLAGS)),
                ("codex --version", exits(2, "codex-cli 0.150.0\n")),
            ],
            refusal: Some(no_queue("Codex 0.150.0")),
            read: true,
        },
        Pair {
            what: "no flags, exit 0",
            pin: None,
            answers: vec![
                ("codex queue --help", Ok(NO_FLAGS.to_owned())),
                version("codex-cli 0.150.0\n"),
            ],
            refusal: Some(no_queue("Codex 0.150.0")),
            read: true,
        },
        Pair {
            what: "both flags, exit 2",
            pin: None,
            answers: vec![("codex queue --help", exits(2, FLAGS))],
            refusal: None,
            read: true,
        },
        Pair {
            // The run ends at its 30 s with its output and exit 0: an answer.
            what: "both flags, from a run that ended at its 30 s",
            pin: None,
            answers: vec![("codex queue --help", Ok(FLAGS.to_owned()))],
            refusal: None,
            read: true,
        },
        Pair {
            what: "a run ended for its time",
            pin: None,
            answers: vec![("codex queue --help", unanswered(true))],
            refusal: Some(
                "could not ask Codex whether it has its native queue: it did not answer in time"
                    .to_owned(),
            ),
            read: false,
        },
        Pair {
            what: "a run ended by a signal",
            pin: None,
            answers: vec![("codex queue --help", unanswered(false))],
            refusal: Some(
                "could not ask Codex whether it has its native queue: Command failed: x\n"
                    .to_owned(),
            ),
            read: false,
        },
    ]
}

fn devin_pairs() -> Vec<Pair> {
    let said = |text: &str| vec![("devin --version", Ok(text.to_owned()))];
    let old = Some(
        "Devin 3000.10.21 or newer is required for complete worker replies. Update Devin before opening this pane."
            .to_owned(),
    );
    vec![
        Pair {
            what: "a supported Devin",
            pin: None,
            answers: said("devin 3000.11.92 (7f4e64980e22)\n"),
            refusal: None,
            read: true,
        },
        Pair {
            what: "an older Devin",
            pin: None,
            answers: said("devin 3000.9.1\n"),
            refusal: old.clone(),
            read: true,
        },
        Pair {
            what: "devin v3000.10.21, which the launch refuses",
            pin: None,
            answers: said("devin v3000.10.21\n"),
            refusal: old.clone(),
            read: true,
        },
        Pair {
            what: "the minimum, with exit 2",
            pin: None,
            answers: vec![("devin --version", exits(2, "devin 3000.10.21\n"))],
            refusal: None,
            read: true,
        },
        Pair {
            // Answered after 4 s: the page's old bound was 3 s.
            what: "a supported answer that comes after 4 s",
            pin: None,
            answers: said("devin 3000.11.92 (7f4e64980e22)\n"),
            refusal: None,
            read: true,
        },
        Pair {
            what: "no version",
            pin: None,
            answers: said("hello\n"),
            refusal: old,
            read: true,
        },
        Pair {
            what: "a run that did not answer",
            pin: None,
            answers: vec![("devin --version", unanswered(false))],
            refusal: Some("Command failed: x\n".to_owned()),
            read: false,
        },
    ]
}

fn muse_pairs() -> Vec<Pair> {
    let line = |version: &str| {
        vec![(
            "muse --version",
            Ok(format!("Muse Code {version} ({version}-R6996.1)\n")),
        )]
    };
    let old = Some(crate::muse::version::required());
    let mut pairs = vec![
        Pair {
            what: "a pin of 1.4.12",
            pin: Some(("1.4.12-R6000", true)),
            answers: Vec::new(),
            refusal: old.clone(),
            read: true,
        },
        Pair {
            what: "a pin of 1.4.23 with its binary",
            pin: Some(("1.4.23-R7000", true)),
            answers: Vec::new(),
            refusal: None,
            read: true,
        },
        Pair {
            what: "no pin, 1.4.12",
            pin: None,
            answers: line("1.4.12"),
            refusal: old.clone(),
            read: true,
        },
        Pair {
            what: "no pin, 1.4.24",
            pin: None,
            answers: line("1.4.24"),
            refusal: None,
            read: true,
        },
        Pair {
            what: "no pin and no version named",
            pin: None,
            answers: vec![("muse --version", Ok("hello\n".to_owned()))],
            refusal: None,
            read: true,
        },
        Pair {
            what: "a pin whose binary is absent, a launcher at 1.4.12",
            pin: Some(("1.4.23-R7000", false)),
            answers: line("1.4.12"),
            refusal: old,
            read: true,
        },
    ];
    // Windows' batch launcher with no pinned binary is refused with no program run: a
    // launcher named `muse.cmd` is found there alone.
    if cfg!(windows) {
        pairs.push(Pair {
            what: "Windows' batch launcher with no pinned binary",
            pin: None,
            answers: Vec::new(),
            refusal: Some("batch".to_owned()),
            read: false,
        });
    }
    pairs
}

/// Sets `pair` in `world`: the pin, the CLI, its answers.
fn lay(world: &World, pair: &Pair) {
    let command = match world.harness {
        Harness::Codex => "codex",
        Harness::Devin => "devin",
        _ => "muse",
    };
    if cfg!(windows) && world.harness == Harness::Muse && pair.what.starts_with("Windows") {
        fs::write(world.bin.join("muse.cmd"), "").unwrap();
    } else {
        world.install(command);
    }
    if let Some((pin, binary)) = pair.pin {
        fs::write(world.bin.join(".muse-version"), format!("{pin}\n")).unwrap();
        if binary {
            let suffix = if cfg!(windows) { ".exe" } else { "" };
            fs::write(world.bin.join(format!("muse-bin-{pin}{suffix}")), "binary").unwrap();
        }
    }
    for (program, answer) in &pair.answers {
        world.answer(program, answer.clone());
    }
}

/// The launch's refusal and the page's verdict of one pair, each in a world of its own.
///
/// A sentence that names the folder of its world says `$BIN` where it does.
fn both(harness: Harness, pair: &Pair) -> (Result<(), String>, Rc<Row>, String) {
    let launch = World::new(harness);
    lay(&launch, pair);
    let in_launch = launch.bin.to_string_lossy().into_owned();
    let prepared = launch
        .prepare()
        .map(|_| ())
        .map_err(|sentence| sentence.replace(&in_launch, "$BIN"));
    let page = World::new(harness);
    lay(&page, pair);
    (
        prepared,
        page.look(false),
        page.bin.to_string_lossy().into_owned(),
    )
}

#[test]
fn the_page_says_what_the_launch_says_of_each_answer_a_cli_gives() {
    let tables = [
        (Harness::Codex, codex_pairs()),
        (Harness::Devin, devin_pairs()),
        (Harness::Muse, muse_pairs()),
    ];
    let mut rows = 0;
    for (harness, pairs) in tables {
        for pair in pairs {
            rows += 1;
            let (prepared, row, in_page) = both(harness, &pair);
            let said = format!("{harness:?}: {}", pair.what);
            match (&pair.refusal, &row.setup) {
                (None, setup) => {
                    assert_eq!(*setup, Some(Setup::Ready), "{said}");
                    assert!(
                        prepared.is_ok() || !is_check_sentence(prepared.as_ref().unwrap_err()),
                        "{said}: the launch is refused by its check: {prepared:?}"
                    );
                }
                (Some(sentence), Some(setup)) => {
                    let (shown, read) = match setup {
                        Setup::UpdateRequired { reason } => (reason, true),
                        Setup::Unchecked { reason } => (reason, false),
                        Setup::Ready => panic!("{said}: the page says Ready"),
                    };
                    assert_eq!(read, pair.read, "{said}");
                    if sentence != "batch" {
                        assert_eq!(shown, sentence, "{said}");
                    }
                    assert_eq!(
                        *prepared.as_ref().unwrap_err(),
                        shown.replace(&in_page, "$BIN"),
                        "{said}: the launch's"
                    );
                }
                (Some(_), None) => panic!("{said}: no verdict"),
            }
        }
    }
    assert_eq!(
        rows,
        if cfg!(windows) { 21 } else { 20 },
        "the Windows batch row is its own"
    );
}

/// Whether `error` is one of the sentences a check refuses with.
fn is_check_sentence(error: &str) -> bool {
    error.contains("native queue")
        || error.contains("or newer is required")
        || error.contains("could not ask")
}

/// How many times each question was asked, of the runs `ran` names.
fn count(ran: &[String], question: &str) -> usize {
    ran.iter().filter(|name| name.as_str() == question).count()
}

fn codex_world(queue: Result<String, Failed>) -> World {
    let world = World::new(Harness::Codex);
    world.install("codex");
    world.always("codex queue --help", queue);
    world.always("codex --version", Ok("codex-cli 0.150.0\n".to_owned()));
    world
}

#[test]
fn a_prepare_that_passes_then_a_look_asks_the_queue_once_and_the_version_once() {
    let world = codex_world(Ok(FLAGS.to_owned()));
    // Past its check; what follows (Codex's own instructions) is not this test's.
    assert!(!is_check_sentence(
        &world.prepare().err().unwrap_or_default()
    ));
    let ran = world.ran();
    assert_eq!(
        (
            count(&ran, "codex queue --help"),
            count(&ran, "codex --version")
        ),
        (1, 0),
        "a launch that passes asks no version"
    );
    assert_eq!(world.look(false).setup, Some(Setup::Ready));
    let ran = world.ran();
    assert_eq!(
        (
            count(&ran, "codex queue --help"),
            count(&ran, "codex --version")
        ),
        (0, 1),
        "the look asks its version, and nothing again"
    );
}

#[test]
fn a_look_then_a_prepare_that_passes_asks_each_question_once() {
    let world = codex_world(Ok(FLAGS.to_owned()));
    assert_eq!(world.look(false).setup, Some(Setup::Ready));
    assert!(!is_check_sentence(
        &world.prepare().err().unwrap_or_default()
    ));
    let ran = world.ran();
    assert_eq!(
        (
            count(&ran, "codex queue --help"),
            count(&ran, "codex --version")
        ),
        (1, 1)
    );
}

#[test]
fn a_look_at_a_codex_with_no_queue_asks_each_question_once_and_the_next_look_asks_none() {
    let world = codex_world(Ok(NO_FLAGS.to_owned()));
    let row = world.look(false);
    assert_eq!(
        row.setup,
        Some(Setup::UpdateRequired {
            reason: no_queue("Codex 0.150.0")
        })
    );
    assert_eq!(row.version.value(), Some("0.150.0"));
    let ran = world.ran();
    assert_eq!(
        (
            count(&ran, "codex queue --help"),
            count(&ran, "codex --version")
        ),
        (1, 1)
    );
    world.look(false);
    assert!(
        world.ran().is_empty(),
        "the same look again asks nothing more"
    );
    // The Codex gains its queue: a look with refresh asks both again, and a prepare none.
    world.always("codex queue --help", Ok(FLAGS.to_owned()));
    world.rewrite("codex", "# updated\n");
    assert_eq!(world.look(true).setup, Some(Setup::Ready));
    let ran = world.ran();
    assert_eq!(
        (
            count(&ran, "codex queue --help"),
            count(&ran, "codex --version")
        ),
        (1, 1)
    );
    world.prepare().ok();
    assert_eq!(
        count(&world.ran(), "codex queue --help"),
        0,
        "the launch reads what the look kept"
    );
}

#[test]
fn a_changed_file_behind_the_same_path_is_seen_at_the_next_look_and_no_verdict_is_kept() {
    let world = codex_world(Ok(NO_FLAGS.to_owned()));
    assert!(matches!(
        world.look(false).setup,
        Some(Setup::UpdateRequired { .. })
    ));
    // The file behind the path is changed for a Codex with its queue.
    world.always("codex queue --help", Ok(FLAGS.to_owned()));
    world.rewrite("codex", "# a newer Codex\n");
    world.prepare().ok();
    assert_eq!(
        world.look(false).setup,
        Some(Setup::Ready),
        "five minutes have not passed, and the page shows what a launch gets now"
    );
}

#[test]
fn a_question_that_failed_and_then_answered_for_a_launch_has_answered_for_the_page() {
    let world = World::new(Harness::Codex);
    world.install("codex");
    world.answer("codex queue --help", unanswered(true));
    world.answer("codex queue --help", Ok(FLAGS.to_owned()));
    assert!(matches!(
        world.look(false).setup,
        Some(Setup::Unchecked { .. })
    ));
    world.prepare().ok();
    assert_eq!(
        world.look(false).setup,
        Some(Setup::Ready),
        "the look inside five minutes shows the launch's answer, not its own failure"
    );
}

/// The Codex fixture over the page's own scripted programs, where a test holds an answer.
fn held_codex() -> Fixture {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture.latest.says(Harness::Codex, "0.171.0");
    fixture.latest.says(Harness::Codex, "0.171.0");
    fixture
}

fn begin_look(driver: &mut Driver<Rc<Row>>, fixture: &Fixture, work: usize, refresh: bool) {
    let admin = Rc::clone(&fixture.admin);
    driver.begin(work, async move {
        admin.check(Some("codex"), refresh).await.unwrap().remove(0)
    });
}

#[test]
fn a_refresh_during_a_question_starts_a_new_one_and_does_not_take_the_old_answer() {
    let fixture = held_codex();
    fixture
        .capture
        .answer("codex queue --help", [Response::Held, Response::Held]);
    fixture
        .capture
        .says("codex --version", "codex-cli 0.150.0\n");
    fixture
        .capture
        .says("codex --version", "codex-cli 0.150.0\n");
    let mut driver = Driver::default();
    begin_look(&mut driver, &fixture, 0, false);
    assert!(driver.run().is_empty());
    begin_look(&mut driver, &fixture, 1, true);
    assert!(driver.run().is_empty());
    let queued = fixture
        .capture
        .take_ran()
        .iter()
        .filter(|(program, _)| named(program) == "codex queue --help")
        .count();
    assert_eq!(queued, 2, "the refresh asked its own question");
    // The refresh's answer, the newer, has the queue; the older says none, and ends last.
    assert!(fixture
        .capture
        .release_newest("codex queue --help", Ok(captured(FLAGS))));
    let settled = driver.run();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].1.setup, Some(Setup::Ready));
    assert!(fixture
        .capture
        .release("codex queue --help", Ok(captured(NO_FLAGS))));
    driver.run();
    assert_eq!(
        fixture.row("codex", false).setup,
        Some(Setup::Ready),
        "the newest look is the answer kept"
    );
}

/// Moves the clock by `ms`, firing the timers due on the way and running the looks.
fn advance(fixture: &Fixture, driver: &mut Driver<Rc<Row>>, ms: i64) -> Vec<(usize, Rc<Row>)> {
    let until = fixture.time.wall_ms() + ms;
    let mut settled = driver.run();
    while fixture.time.fire_next(until) {
        settled.extend(driver.run());
    }
    fixture.time.settle_at(until);
    settled
}

fn cut_off(killed: bool) -> Result<Captured, CaptureFailed> {
    Err(CaptureFailed {
        message: "Command failed: x\n".to_owned(),
        code: None,
        killed,
        stdout: String::new(),
        stderr: String::new(),
    })
}

#[test]
fn two_questions_of_29_seconds_give_the_row_at_29_seconds_not_at_their_sum() {
    let fixture = held_codex();
    fixture
        .capture
        .answer("codex queue --help", [Response::Held]);
    fixture.capture.answer("codex --version", [Response::Held]);
    let mut driver = Driver::default();
    begin_look(&mut driver, &fixture, 0, false);
    assert!(advance(&fixture, &mut driver, 29_000).is_empty());
    assert!(fixture
        .capture
        .release("codex queue --help", Ok(captured(NO_FLAGS))));
    assert!(fixture
        .capture
        .release("codex --version", Ok(captured("codex-cli 0.150.0\n"))));
    let settled = advance(&fixture, &mut driver, 0);
    assert_eq!(fixture.time.wall_ms(), EPOCH_MS + 29_000);
    let (_, row) = &settled[0];
    assert_eq!(
        row.setup,
        Some(Setup::UpdateRequired {
            reason: no_queue("Codex 0.150.0")
        })
    );
}

#[test]
fn a_question_that_never_ends_leaves_the_row_unchecked_at_40_seconds_and_the_others_whole() {
    let fixture = Fixture::new();
    for command in ["claude", "codex", "pi"] {
        fixture.install("bin", command);
        fixture
            .capture
            .says(&format!("{command} --version"), "5.0.0");
        fixture
            .latest
            .says(Harness::from_name(command).unwrap(), "5.0.1");
    }
    fixture
        .capture
        .answer("codex queue --help", [Response::Held]);
    fixture.latest.says(Harness::Codex, "5.0.1");
    let mut driver = Driver::default();
    let admin = Rc::clone(&fixture.admin);
    driver.begin(0, async move { admin.check(None, false).await.unwrap() });
    let mut settled = Vec::new();
    let until = EPOCH_MS + 40_000;
    settled.extend(driver.run());
    while fixture.time.fire_next(until) {
        settled.extend(driver.run());
    }
    assert_eq!(fixture.time.wall_ms(), until);
    let rows = &settled[0].1;
    let by_id = |id: Harness| rows.iter().find(|row| row.id == id).unwrap();
    assert_eq!(
        by_id(Harness::Codex).setup,
        Some(Setup::Unchecked {
            reason: "ConsensFlow could not finish checking codex in 40 seconds".to_owned()
        })
    );
    for id in [Harness::Claude, Harness::Pi] {
        assert!(matches!(by_id(id).update, Update::Available(_)), "{id:?}");
        assert_eq!(by_id(id).version.value(), Some("5.0.0"));
    }
}

#[test]
fn a_version_that_failed_early_is_not_asked_again_when_the_queue_answers_late() {
    // The version question is unanswered at 1 s; no queue is said at 29 s. The refusal
    // names no version, was reached at 29 s, and the version was asked once.
    let fixture = held_codex();
    fixture
        .capture
        .answer("codex queue --help", [Response::Held]);
    fixture.capture.answer("codex --version", [Response::Held]);
    let mut driver = Driver::default();
    begin_look(&mut driver, &fixture, 0, false);
    advance(&fixture, &mut driver, 1_000);
    assert!(fixture.capture.release("codex --version", cut_off(true)));
    assert!(advance(&fixture, &mut driver, 28_000).is_empty());
    assert!(fixture
        .capture
        .release("codex queue --help", Ok(captured(NO_FLAGS))));
    let settled = advance(&fixture, &mut driver, 0);
    assert_eq!(fixture.time.wall_ms(), EPOCH_MS + 29_000);
    let (_, row) = &settled[0];
    assert_eq!(
        row.setup,
        Some(Setup::UpdateRequired {
            reason: no_queue("This Codex")
        })
    );
    assert_eq!(
        row.version,
        Version::Error {
            reason: "Version check timed out".to_owned()
        }
    );
    let versions = fixture
        .capture
        .take_ran()
        .iter()
        .filter(|(program, _)| named(program) == "codex --version")
        .count();
    assert_eq!(versions, 1, "one version question for the look");
}

#[test]
fn a_refusal_already_known_survives_a_version_that_never_answers() {
    let fixture = held_codex();
    fixture
        .capture
        .answer("codex queue --help", [Response::Held]);
    fixture.capture.answer("codex --version", [Response::Held]);
    let mut driver = Driver::default();
    begin_look(&mut driver, &fixture, 0, false);
    advance(&fixture, &mut driver, 29_000);
    assert!(fixture
        .capture
        .release("codex queue --help", Ok(captured(NO_FLAGS))));
    let settled = advance(&fixture, &mut driver, 11_000);
    assert_eq!(settled.len(), 1);
    assert!(
        fixture.time.wall_ms() <= EPOCH_MS + 40_000,
        "answered by the cut"
    );
    assert_eq!(
        settled[0].1.setup,
        Some(Setup::UpdateRequired {
            reason: no_queue("This Codex")
        }),
        "the queue's refusal is not lost to the cut"
    );
}

#[cfg(unix)]
#[allow(clippy::disallowed_methods)] // The test makes the pipe it is about.
#[test]
fn a_pipe_in_the_place_of_muses_pin_does_not_hold_the_look() {
    let world = World::new(Harness::Muse);
    world.install("muse");
    let pin = world.bin.join(".muse-version");
    let made = std::process::Command::new("mkfifo")
        .arg(&pin)
        .status()
        .unwrap();
    assert!(made.success());
    world.answer(
        "muse --version",
        Ok("Muse Code 1.4.24 (1.4.24-R7000.1)\n".to_owned()),
    );
    let row = world.look(false);
    assert_eq!(row.setup, Some(Setup::Ready));
    assert_eq!(row.version.value(), Some("1.4.24-R7000.1"));
}

#[test]
fn the_look_keeps_no_verdict_but_the_feeds_answer_five_minutes() {
    let world = codex_world(Ok(FLAGS.to_owned()));
    let first = world.look(false);
    assert_eq!(first.setup, Some(Setup::Ready));
    let at = Duration::from_secs(60);
    let time: &ManualTime = &world.fakes.time;
    time.settle_at(EPOCH_MS + i64::try_from(at.as_millis()).unwrap());
    assert!(
        !Rc::ptr_eq(&first, &world.look(false)),
        "a look is never the row kept"
    );
    assert_eq!(world.look(false).checked_at, EPOCH_MS + 60_000);
}

#[test]
fn the_page_shows_a_refusal_or_nothing_and_never_ready_for_what_a_launch_brings() {
    // A Muse that passes its check is Ready: the page draws nothing for it.
    let world = World::new(Harness::Muse);
    world.install("muse");
    world.answer(
        "muse --version",
        Ok("Muse Code 1.4.24 (1.4.24-R7000.1)\n".to_owned()),
    );
    let json = serde_json::to_value(&*world.look(false)).unwrap();
    assert_eq!(json["setup"]["state"], "ready");
    assert!(json["setup"].get("reason").is_none());
}
