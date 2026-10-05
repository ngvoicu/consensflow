//! What a Codex launch asks of its CLI and draws for its broker: the native
//! queue and the version a refusal names, the port and the token, and the
//! supervisor the window opens on.

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::env::Env;
use serde_json::Value;
use tempfile::TempDir;

use super::*;
use crate::codex::fakes::Watching;
use crate::contract::Work;
use crate::seams::loopback::Request;
use crate::seams::processes::{Child, Failed, Limits, Processes, Program, Streams};
use crate::seams::{Entropy, Ports};
use crate::testing::{fake_executable, finished, Fakes, ScriptedProcesses};

const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
const HELP: &str = "Usage: codex queue --thread <id> --message <text>\n";

#[test]
fn a_flag_ends_a_word_as_javascript_s_word_boundary_ends_it() {
    for (text, ends) in [
        ("--thread", true),
        ("--thread <id>", true),
        ("--thread-id", true),
        ("--thread=7", true),
        ("--threads --thread", true),
        // A letter of another alphabet and a digit of another script are no
        // JavaScript word: the boundary is there.
        ("--thread\u{e9}", true),
        ("--thread\u{660}", true),
        ("--threads", false),
        ("--thread_id", false),
        ("--thread1", false),
        ("--threadZ", false),
        ("thread", false),
        ("", false),
    ] {
        assert_eq!(ends_a_word(text, "--thread"), ends, "{text:?}");
    }
}

#[test]
fn a_version_is_the_first_number_of_three_parts_and_what_sticks_to_it() {
    for (text, version) in [
        ("codex-cli 0.150.0", Some("0.150.0")),
        ("codex-cli 0.150.0\n", Some("0.150.0")),
        (
            "node 20.1 codex 1.2.3-beta.1+build.7 (extra)",
            Some("1.2.3-beta.1+build.7"),
        ),
        ("v10.20.30.40", Some("10.20.30.40")),
        ("a.1.2.3", Some("1.2.3")),
        ("1.2.x 3.4.5", Some("3.4.5")),
        ("1.2.3.", Some("1.2.3.")),
        ("1.2", None),
        ("1.2.", None),
        ("1..3", None),
        ("codex", None),
        ("", None),
        // It ends at a JavaScript space: a no-break space and a byte order
        // mark are two, a next-line character is none.
        ("1.2.3\u{a0}y 4.5.6", Some("1.2.3")),
        ("1.2.3\u{feff}y", Some("1.2.3")),
        ("1.2.3\u{85}y z", Some("1.2.3\u{85}y")),
        // Its digits are ASCII.
        ("\u{660}.\u{661}.\u{662}", None),
        ("\u{660}1.2.3", Some("1.2.3")),
    ] {
        assert_eq!(version_in(text), version, "{text:?}");
    }
}

/// A CLI in a folder of its own, scripted programs, and the services of both.
struct Stage {
    _dir: TempDir,
    cli: String,
    fakes: Fakes,
    watching: Rc<Watching>,
    services: Services,
}

impl Stage {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_executable(&dir.path().join("codex"));
        let env = Env::default();
        let fakes = Fakes::new(&env);
        let watching = Rc::new(Watching::default());
        let mut services = fakes.services(&env, dir.path());
        services.processes = Rc::clone(&watching) as Rc<dyn Processes>;
        Self {
            _dir: dir,
            cli: cli.to_string_lossy().into_owned(),
            fakes,
            watching,
            services,
        }
    }

    /// The CLI answers `arguments` with `answer`, once.
    fn answers(&self, arguments: &str, answer: Result<String, Failed>) {
        self.watching
            .scripted
            .run_answer(&format!("codex {arguments}"), answer);
    }

    fn configure(&self) -> Result<Launched, String> {
        finished(Box::pin(configuration(
            &self.services,
            &LaunchId::new(LAUNCH).unwrap(),
            "/work/app",
            &self.cli,
        )))
    }

    fn require(&self) -> Result<(), String> {
        finished(Box::pin(require_native_queue(&self.services, &self.cli)))
    }

    /// The request the broker of `channel` is first asked with, which nobody answers.
    fn asked(&self, channel: &Channel) -> Request {
        let services = &self.services;
        finished(Box::pin(
            channel.shown(&*services.time, &*services.loopback),
        ));
        self.fakes.loopback.take_asked().remove(0)
    }
}

/// A program that ended as `code` says, or did not answer.
fn failed(code: Option<i32>, killed: bool, stdout: &str) -> Failed {
    Failed {
        message: "Command failed: codex queue --help\nboom\n".to_owned(),
        code,
        killed,
        stdout: stdout.to_owned(),
    }
}

#[test]
fn a_cli_with_the_native_queue_is_asked_once_however_many_launches_there_are() {
    let stage = Stage::new();
    stage.answers("queue --help", Ok(HELP.to_owned()));
    stage.configure().unwrap();
    stage.configure().unwrap();
    assert_eq!(stage.watching.scripted.take_ran(), ["codex queue --help"]);
}

#[test]
fn a_cli_without_the_native_queue_is_refused_by_its_version() {
    for (help, version, named) in [
        (
            Ok("Usage: codex\n".to_owned()),
            Ok("codex-cli 0.150.0\n".to_owned()),
            "Codex 0.150.0",
        ),
        // An exit with a code is an answer, and what it said is read.
        (
            Err(failed(Some(2), false, "")),
            Err(failed(Some(1), false, "codex 9.9.9\n")),
            "Codex 9.9.9",
        ),
        (
            Err(failed(Some(2), false, "")),
            Ok("codex\n".to_owned()),
            "This Codex",
        ),
        // A version that could not be asked for is none.
        (
            Err(failed(Some(2), false, "")),
            Err(failed(None, true, "")),
            "This Codex",
        ),
        (
            Err(failed(Some(2), false, "")),
            Err(failed(None, false, "1.2.3")),
            "This Codex",
        ),
    ] {
        let stage = Stage::new();
        stage.answers("queue --help", help);
        stage.answers("--version", version);
        assert_eq!(
            stage.require(),
            Err(format!(
                "{named} has no native queue, which ConsensFlow needs to reach its window: update Codex."
            ))
        );
    }
}

#[test]
fn a_help_with_one_of_the_flags_or_a_longer_word_is_no_native_queue() {
    for help in [
        "--thread only",
        "--message only",
        "--threads --messages",
        "--thread --message_text",
        "--thread1 --message",
    ] {
        let stage = Stage::new();
        stage.answers("queue --help", Ok(help.to_owned()));
        stage.answers("--version", Ok("0.1.2".to_owned()));
        assert!(stage.require().is_err(), "{help:?}");
    }
    for help in [
        "--thread-id --message-text",
        "--thread\u{e9} --message\u{660}",
        "--message --thread",
    ] {
        let stage = Stage::new();
        stage.answers("queue --help", Ok(help.to_owned()));
        assert_eq!(stage.require(), Ok(()), "{help:?}");
    }
}

#[test]
fn a_cli_that_did_not_answer_in_time_says_so_and_one_that_failed_says_why() {
    let stage = Stage::new();
    stage.answers("queue --help", Err(failed(None, true, "")));
    assert_eq!(
        stage.require(),
        Err(
            "could not ask Codex whether it has its native queue: it did not answer in time"
                .to_owned()
        )
    );
    // Nothing that did not answer is kept: it is asked again.
    stage.answers("queue --help", Err(failed(None, false, "")));
    assert_eq!(
        stage.require(),
        Err("could not ask Codex whether it has its native queue: Command failed: codex queue --help\nboom\n".to_owned())
    );
    stage.answers("queue --help", Ok(HELP.to_owned()));
    assert_eq!(stage.require(), Ok(()));
    assert_eq!(stage.watching.scripted.take_ran().len(), 3);
}

#[test]
fn a_cli_that_cannot_be_looked_at_is_refused_in_the_words_of_the_system() {
    let stage = Stage::new();
    let refused = finished(Box::pin(require_native_queue(
        &stage.services,
        &stage.cli.replace("codex", "nowhere"),
    )))
    .unwrap_err();
    assert!(
        refused.starts_with("ENOENT: no such file or directory, realpath"),
        "{refused}"
    );
    assert!(
        stage.watching.scripted.take_ran().is_empty(),
        "nothing was run"
    );
}

/// Programs that take the CLI away once they have run anything.
struct Vanishing {
    file: String,
    scripted: ScriptedProcesses,
}

impl Processes for Vanishing {
    fn run(&self, program: Program, limits: Limits) -> Work<'_, Result<String, Failed>> {
        let answer = self.scripted.run(program, limits);
        std::fs::remove_file(&self.file).unwrap();
        answer
    }

    fn spawn(&self, program: Program, streams: Streams) -> Result<Box<dyn Child>, String> {
        self.scripted.spawn(program, streams)
    }
}

#[test]
fn a_cli_that_vanishes_before_its_version_is_asked_is_refused_in_the_words_of_the_system() {
    let stage = Stage::new();
    let vanishing = Rc::new(Vanishing {
        file: stage.cli.clone(),
        scripted: ScriptedProcesses::default(),
    });
    vanishing
        .scripted
        .run_answer("codex queue --help", Ok("Usage: codex\n".to_owned()));
    let mut services = stage.services.clone();
    services.processes = Rc::clone(&vanishing) as Rc<dyn Processes>;
    let refused = finished(Box::pin(require_native_queue(&services, &stage.cli))).unwrap_err();
    assert!(
        refused.starts_with("ENOENT: no such file or directory, realpath"),
        "{refused}"
    );
}

#[test]
fn a_cli_that_is_not_at_an_absolute_path_is_refused_before_it_is_asked() {
    let stage = Stage::new();
    let refused = finished(Box::pin(require_native_queue(&stage.services, "codex")));
    assert_eq!(
        refused,
        Err("a Codex launch needs the absolute path of its CLI".to_owned())
    );
    assert!(stage.watching.scripted.take_ran().is_empty());
}

#[test]
fn a_launch_with_no_workspace_is_refused_before_its_cli_is_asked() {
    let stage = Stage::new();
    let launch = LaunchId::new(LAUNCH).unwrap();
    let refused = finished(Box::pin(configuration(
        &stage.services,
        &launch,
        "",
        &stage.cli,
    )));
    assert_eq!(
        refused.err(),
        Some("launch configuration needs a workspace".to_owned())
    );
    assert!(stage.watching.scripted.take_ran().is_empty());
}

#[test]
fn a_broker_is_the_next_free_port_on_loopback_and_a_token_of_twenty_four_bytes() {
    let stage = Stage::new();
    stage.answers("queue --help", Ok(HELP.to_owned()));
    let launched = stage.configure().unwrap();
    // The scripted stream's first 24 bytes, `(i * 7 + 3) % 256`, as base64url.
    let token = "AwoRGB8mLTQ7QklQV15lbHN6gYiPlp2k";
    let request = stage.asked(&launched.channel);
    assert_eq!(request.url, "http://127.0.0.1:41000/session");
    assert_eq!(
        request.headers,
        [("authorization".to_owned(), format!("Bearer {token}"))]
    );
    assert_eq!(stage.fakes.entropy.take_draws(), [24]);
    let second = stage.configure().unwrap();
    assert_eq!(
        stage.asked(&second.channel).url,
        "http://127.0.0.1:41001/session"
    );
}

/// Randomness that is the same byte, and the size of what it was asked for.
struct Same(u8, RefCell<Vec<usize>>);

impl Entropy for Same {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), String> {
        bytes.fill(self.0);
        self.1.borrow_mut().push(bytes.len());
        Ok(())
    }
}

#[test]
fn a_token_is_written_in_the_url_safe_alphabet_with_no_padding() {
    let mut stage = Stage::new();
    stage.answers("queue --help", Ok(HELP.to_owned()));
    stage.services.entropy = Rc::new(Same(0xfb, RefCell::default()));
    let launched = stage.configure().unwrap();
    let token = "-_v7".repeat(8);
    assert_eq!(
        stage.asked(&launched.channel).headers,
        [("authorization".to_owned(), format!("Bearer {token}"))]
    );
    assert_eq!(launched.env[0].1.matches(&token).count(), 1);
}

#[test]
fn the_supervisor_is_told_its_broker_by_one_variable_in_the_order_node_wrote_it() {
    let stage = Stage::new();
    stage.answers("queue --help", Ok(HELP.to_owned()));
    let launched = stage.configure().unwrap();
    let [(name, bridge)] = &launched.env[..] else {
        panic!("{:?}", launched.env);
    };
    assert_eq!(name, "CF_CODEX_SESSION_BRIDGE");
    let told: Value = serde_json::from_str(bridge).unwrap();
    assert_eq!(told["launchId"], LAUNCH);
    assert_eq!(told["port"], 41000);
    assert_eq!(
        bridge,
        &format!(
            r#"{{"launchId":"{LAUNCH}","port":41000,"token":"AwoRGB8mLTQ7QklQV15lbHN6gYiPlp2k"}}"#
        )
    );
}

/// Ports the system would not give.
struct NoPort;

impl Ports for NoPort {
    fn free_loopback(&self) -> Result<u16, String> {
        Err("could not choose a loopback port".to_owned())
    }
}

/// Randomness the system would not give.
struct NoRandomness;

impl Entropy for NoRandomness {
    fn fill(&self, _bytes: &mut [u8]) -> Result<(), String> {
        Err("no randomness".to_owned())
    }
}

#[test]
fn a_port_or_randomness_the_system_would_not_give_stops_the_launch_in_its_words() {
    let mut stage = Stage::new();
    stage.answers("queue --help", Ok(HELP.to_owned()));
    stage.services.ports = Rc::new(NoPort);
    assert_eq!(
        stage.configure().err(),
        Some("could not choose a loopback port".to_owned())
    );
    assert_eq!(
        stage.fakes.entropy.take_draws(),
        Vec::<usize>::new(),
        "the port comes before the token"
    );
    let mut stage = Stage::new();
    stage.answers("queue --help", Ok(HELP.to_owned()));
    stage.services.entropy = Rc::new(NoRandomness);
    assert_eq!(stage.configure().err(), Some("no randomness".to_owned()));
}

#[test]
fn the_native_queue_is_asked_before_a_port_is_chosen_or_a_token_drawn() {
    let stage = Stage::new();
    stage.answers("queue --help", Ok("Usage: codex\n".to_owned()));
    stage.answers("--version", Ok("codex 1.0.0".to_owned()));
    assert!(stage.configure().is_err());
    assert_eq!(stage.fakes.entropy.take_draws(), Vec::<usize>::new());
    assert_eq!(
        stage.fakes.ports.0.borrow().front(),
        Some(&41_000),
        "no port was taken"
    );
}

#[test]
fn a_window_opens_on_the_bundle_s_cf_as_the_supervisor_given_codex_and_then_its_arguments() {
    let stage = Stage::new();
    let args = vec!["-c".to_owned(), "x=1".to_owned()];
    let argv = with_native_bridge(&stage.services.bundle, "/usr/bin/codex", args);
    assert_eq!(
        argv,
        [
            stage.services.bundle.cf.to_string_lossy().into_owned(),
            "codex-session".to_owned(),
            "/usr/bin/codex".to_owned(),
            "-c".to_owned(),
            "x=1".to_owned(),
        ]
    );
}
