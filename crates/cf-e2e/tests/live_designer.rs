//! A real Codex draws a real image for a chief, through ConsensFlow: the proof
//! that the image designer works, which unit, catalog and page tests cannot give.
//! It is opt-in (`cargo xtask live designer`, `npm run live:designer`): it spends
//! the machine's Codex quota and needs the machine's logged-in Codex, so it is
//! ignored unless asked for and is no part of `check` or of CI.
//!
//! The native daemon and the real pane host run in a home of the run's own
//! (never `~/.consensflow`), with the machine's `codex` on the PATH of their
//! windows and its login read where it is (`CODEX_HOME`: the window reads it, and
//! writes its sessions there as it does for the person it belongs to; nothing is
//! copied). The project is a folder in that home (`git init`, a README, [`demo`])
//! whose staff is the catalog's image designer, `pygmalion`, alone. The chief is a
//! stand-in Claude that only notes what it is sent, and the test plays the chief
//! with the chief's own token, as the rig's cases do: `cf task add --design`.
//!
//! Then it waits ([`waits`]), up to ten minutes and saying what it waits for,
//! for the designer's result to reach the chief, and holds what came of it to
//! the brief ([`verdict`]): the file is where the task said and is a whole PNG
//! of a sane size, the result names it ([`result`]), the task is done on the
//! board, and the designer's window closed. A failed run says what the product
//! showed ([`report`]): the board, the chief's inbox, the task and what the
//! designer's window recorded of it, its screen and the daemon's log, beside the
//! rig's own report.
//!
//! Every run that passes keeps the image it drew where cargo keeps the scratch
//! of integration tests (`app/src-tauri/target/tmp/live-designer/honey.png`), the
//! last over the one before, for a look. `CONSENSFLOW_LIVE_KEEP=1` (`cargo xtask
//! live designer --keep`) leaves the home and the project where they are.

// The parts are in `live_designer/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
#[path = "live_designer/demo.rs"]
mod demo;
#[path = "live_designer/report.rs"]
mod report;
#[path = "live_designer/result.rs"]
mod result;
#[path = "live_designer/verdict.rs"]
mod verdict;
#[path = "live_designer/waits.rs"]
mod waits;

use std::fmt::Display;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use cf_e2e::rig::{Project, Rig, OPEN};
use cf_e2e::{files, live, pattern, Error, Result};
use regex::Regex;
use serde_json::{json, Value};

use result::{named, Named};
use verdict::{Made, Seen};
use waits::{await_result, await_window, wait, CLOSES};

/// The stand-in for the chief's window, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the chief asks the image designer for.
const BRIEF: &str = "Draw a small flat icon of a jar of honey on a plain background. \
                     Save it as images/honey.png.";

/// Where the brief says to save it, from the project's folder, with `/` between folders.
const IMAGE: &str = "images/honey.png";

/// What the run says as it goes, on the error output (which a test's capture
/// does not hold back), with the seconds since it began.
struct Log {
    started: Instant,
}

impl Log {
    fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    fn say(&self, text: impl Display) {
        let seconds = self.started.elapsed().as_secs_f64();
        let _ = writeln!(io::stderr(), "[{seconds:>6.1} s] live designer: {text}");
    }

    /// `title`, then `body` a line at a time, indented.
    fn block(&self, title: &str, body: &str) {
        let indented: Vec<String> = body.lines().map(|line| format!("    {line}")).collect();
        self.say(format!("{title}:\n{}", indented.join("\n")));
    }
}

/// A JSON string as the text it is; anything else as JSON writes it.
fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// The number of the task `cf task add --design` says it put on the board for
/// an image designer, from what it printed.
fn task_for_a_designer(said: &str) -> Option<i64> {
    static BOARD: OnceLock<Regex> = OnceLock::new();
    pattern::once(&BOARD, r"^T-(\d+) is on the board for an image designer;")
        .captures(said.trim())
        .and_then(|found| found[1].parse().ok())
}

/// The chief asks the image designer for the image, as it does in its window:
/// `cf task add --design`, with its own token. The task's number.
fn ask(rig: &Rig, chief: &Value) -> Result<i64> {
    let ran = rig.cf_in_window(chief, ["task", "add", "--design", BRIEF])?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!(
            "cf task add --design was refused: {ran}"
        )));
    }
    task_for_a_designer(&ran.stdout).ok_or_else(|| {
        Error::Daemon(format!(
            "cf task add --design did not say the task is on the board for an image designer: {ran}"
        ))
    })
}

/// Where the brief says the image is, in the project's folder `folder`.
fn image_in(folder: &Path) -> PathBuf {
    IMAGE
        .split('/')
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}

/// Where the image a run drew is kept for a look, in the scratch folder cargo
/// gives the integration tests: the run's own home goes with the run, and the
/// last image drawn is the one here.
fn kept_image() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("live-designer")
        .join("honey.png")
}

/// What came of the run, for the line that ends it.
struct Drawn {
    task: i64,
    made: Made,
    naming: Named,
    exit: Value,
}

/// Looks at what the designer did, and holds it to the brief: every way it
/// fell short, said together, or what it made.
fn judge(
    rig: &Rig,
    project: &Project,
    folder: &Path,
    task: i64,
    window: &Value,
    result: &Value,
    log: &Log,
) -> Result<Drawn> {
    let file = image_in(folder);
    let image = files::read(&file).ok();
    let body = text(&result["body"]);
    let naming = named(&body, &file, IMAGE);
    let state = text(&project.task(task)?["state"]);
    // The window is opened in the project's folder: the image is saved there.
    let opened_in = text(&window["cwd"]);
    let in_the_project =
        std::fs::canonicalize(&opened_in).ok() == std::fs::canonicalize(folder).ok();
    let id = window["id"].clone();
    let exit_of = || rig.exits().into_iter().find(|exit| exit["id"] == id);
    let closed = wait(rig, log, "the designer's window to close", CLOSES, || {
        Ok(exit_of().is_some())
    })
    .is_ok();

    let held = demo::holds(folder);
    let seen = Seen {
        image_at: IMAGE,
        image: image.as_deref(),
        folder: &held,
        result: &body,
        naming,
        task,
        state: &state,
        opened_in: &opened_in,
        in_the_project,
        closed,
        closing: CLOSES.as_secs(),
    };
    match verdict::judge(&seen) {
        Ok(made) => Ok(Drawn {
            task,
            made,
            naming,
            exit: exit_of().unwrap_or(Value::Null),
        }),
        Err(problems) => Err(Error::Daemon(format!(
            "the designer's work is not what the brief asked for:\n  - {}",
            problems.join("\n  - ")
        ))),
    }
}

/// The whole run, from the chief's ask to the judgement of what came of it. The
/// task asked for is said in `asked` as soon as there is one.
fn attempt(
    rig: &Rig,
    project: &Project,
    folder: &Path,
    log: &Log,
    asked: &mut Option<i64>,
) -> Result<Drawn> {
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    log.say("the chief's window is open: the chief asks the image designer for the image");
    let task = ask(rig, &chief)?;
    *asked = Some(task);
    log.say(format!("T-{task} is on the board for an image designer"));
    let window = await_window(rig, project, task, log)?;
    let result = await_result(rig, project, task, &window, log)?;
    log.say(format!(
        "the result reached the chief: {}",
        report::cut(&text(&result["body"]), 300)
    ));
    judge(rig, project, folder, task, &window, &result, log)
}

#[test]
#[ignore = "opt-in: spends the machine's Codex quota; cargo xtask live designer (npm run live:designer)"]
fn a_real_codex_draws_the_image_a_chief_asks_the_image_designer_for() -> Result {
    let codex = live::codex()?;
    let log = Log::new();
    log.say(format!(
        "Codex is {}; its login is read from {}",
        codex.command().display(),
        codex.home().display()
    ));
    let mut rig = Rig::start(codex.rig(FAKE_AGENT))?;
    if live::keep_requested() {
        log.say(format!(
            "{} is set: the home and the project stay at {}",
            live::KEEP,
            rig.keep_home().display()
        ));
    }
    let folder = rig.workspace().join("honey-shop");
    demo::project(&folder)?;
    let project = Project::open(
        &rig,
        "chief",
        json!({
            "directory": folder,
            "staff": [{ "agent": "pygmalion", "roles": ["designer"] }],
        }),
    )?;
    log.say(format!(
        "project {} is open in {}, with the image designer pygmalion for its staff",
        project.id(),
        folder.display()
    ));

    let mut asked = None;
    let drawn = match attempt(&rig, &project, &folder, &log, &mut asked) {
        Ok(drawn) => drawn,
        Err(failed) => {
            report::failure(&rig, &project, asked, &log);
            return Err(failed);
        }
    };
    log.block(
        "what the designer's window recorded",
        &report::transcript(&rig, &project, drawn.task),
    );
    log.block(
        "the designer's window, as its screen ended",
        &report::screen(&rig, &project),
    );
    demo::notes(&rig.log(), &folder, &log);
    files::copy(&image_in(&folder), &kept_image())?;
    let naming = match drawn.naming {
        Named::Whole => "its whole path",
        _ => "only the path the task gave, not the whole path the role text asks for",
    };
    log.say(format!(
        "ok: T-{} is done: {IMAGE} is a {} by {} PNG of {} bytes, kept at {}; the result names \
         {naming}; the designer's window closed (exit code {}, signal {})",
        drawn.task,
        drawn.made.picture.width,
        drawn.made.picture.height,
        drawn.made.bytes,
        kept_image().display(),
        drawn.exit["exitCode"],
        drawn.exit["signal"],
    ));
    rig.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_put_on_the_board_for_a_designer_is_the_one_the_chief_is_told() {
        let said = "T-12 is on the board for an image designer; the first free one gets it, \
                    and its result arrives in your inbox.\n";
        assert_eq!(task_for_a_designer(said), Some(12));
        // A task for a worker is no designer's; a refusal and nothing are none.
        let worker = "T-3 is on the board for a standard worker; the first free one gets it.";
        assert_eq!(task_for_a_designer(worker), None);
        assert_eq!(
            task_for_a_designer("cf: no image designer on the staff"),
            None
        );
        assert_eq!(task_for_a_designer(""), None);
    }

    #[test]
    fn a_json_string_is_the_text_it_is_and_anything_else_is_as_json_writes_it() {
        assert_eq!(text(&json!("a b")), "a b");
        assert_eq!(text(&json!(7)), "7");
        assert_eq!(text(&json!(null)), "null");
        assert_eq!(text(&json!({ "a": 1 })), r#"{"a":1}"#);
    }
}
