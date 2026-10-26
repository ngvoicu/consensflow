//! Live: what Claude Code's record holds of a paste that carries the path of an image, and
//! whether the daemon confirms it. It is opt-in (`cargo xtask live image-paste`, `npm run
//! live:image-paste`): it spends the machine's quota on Claude Code, and needs it installed
//! and logged in, so it is ignored unless asked for and is no part of `check` or of CI.
//!
//! The field bug it answers (2026-10-21, ConsensFlow 3.0.5, `sites`, Claude Code 2.1.306,
//! T-215): an image designer's result starts with the absolute path of each file it saved,
//! and Claude turns a pasted path to an image into an attachment. The record's user item
//! then began with `[Image #4]` and not with the message's header, and the paste was never
//! confirmed: pasted again twice, then failed, the chief having had each result three times.
//!
//! And since T-763 (2026-10-26) the other way a TUI rewrites a paste into a shape the proof must
//! know: Claude Code, given a long paste in pieces when the machine is loaded, wraps each piece
//! and leaves the last bare after the last closing tag. The same runs are made on Devin, to find
//! out whether its TUI converts an image path or splits a long paste; Codex, Pi and OpenCode take
//! their messages through a channel and are never pasted into.
//!
//! Five tests:
//!
//! - **`claude_records_each_shape_of_a_paste_that_names_images_in_a_way_the_engine_takes_for_it`**
//!   (and **`devin_records_each_shape…`**, which holds Devin to the engine taking an item of its
//!   record for the paste, and to making no attachment of any path: its TUI made none, 2026-10-26)
//!   is the probe. Each shape of a paste (one, two and three image paths, at the start, in
//!   the middle and on their own lines, a path to a file that is no image, one to an image that
//!   is not there) goes into one Claude window the way the daemon pastes (`pane.write_paste`,
//!   then Enter), one after another, and the record says what the window made of it: the items
//!   the daemon reads, and the user records as Claude wrote them, each part by its kind. It
//!   holds the record to two things: the engine takes an item of it for the paste of the
//!   message (`is_paste_of`, asked of the engine itself), and Claude made as many attachments
//!   of the paths as the shape names images it can read (the placeholders that begin the item:
//!   what `WINDOWS_CONVERTS` answers where the machine is Windows, whose native paths, a
//!   drive letter and backslashes, are the question of T-265). A last window is given a
//!   message with an image path as the first message of its launch, an argument of the program,
//!   which Claude makes no attachment of. What the shapes are is the report, and where Claude moves
//!   a placeholder or a separator, the run says so first.
//! - **`a_result_that_starts_with_an_image_path_is_delivered_to_a_claude_chief_once`** is the
//!   field's shape (`daemon`): a real Claude as the chief of a project on the native daemon, the
//!   stand-in as the designer, whose result starts with the paths of two pictures it saved. The
//!   result is pasted once, confirmed, and in the chief's record once, with its pictures.
//!   **`…_to_a_devin_chief_once`** is the same with a Devin chief.
//! - **`a_long_result_is_delivered_to_a_claude_chief_once`** is T-763's second shape (`daemon`):
//!   the stand-in's result is a report of 12,000 characters, which is to be pasted once and
//!   confirmed on the first attempt. Run it under load (`python3 tools/under-load.py --burners 28
//!   …` of `wt/load-timing-flakes`) as the field was: there Claude Code was given the paste in
//!   pieces, each wrapped, and the last left bare after the last closing tag (load 37 to 59 and
//!   passes of the daemon of 28 to 60 s). The run says how many pieces, and how many characters
//!   are bare, of the item the chief's record holds, and does not hold it to any: on 2026-10-26
//!   it was one piece in seven runs at 28 and 56 burners (load 40 to 113), and stopping and
//!   starting the pane host to starve its writes did not bring the pieces about either, so the
//!   loose tail is proved by the field's own items (`delivery_text::tests`, `paste_shapes`), and
//!   this run by the delivery staying confirmed on the first attempt under load.
//!
//! ```text
//! cargo xtask live image-paste                      all five
//! cargo xtask live image-paste -- --list            lists them
//! cargo xtask live image-paste -- long              the long result alone, as the filter picks it
//! cargo xtask live image-paste --keep               leaves the daemon run's home and project
//! ```

// The parts are in `live_image_paste/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "live_image_paste/daemon.rs"]
mod daemon;

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::ask::{self, Item};
use cf_e2e::live::claude;
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window, ANSWER};
use cf_e2e::live::{self, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{files, Result};
use serde_json::{json, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it (each is opened on the real harness's own
/// path), but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The files a shape names, in the folder of the window: three images, a text file,
/// and an image that is not there.
const IMAGES: [&str; 3] = ["desk.png", "hero.png", "icon.png"];
const TEXT: &str = "notes.txt";
const MISSING: &str = "gone.png";
const SPACED: &str = "my images/desk art.png";

/// A shape of a paste, and the sum its ask holds.
struct Shape {
    name: &'static str,
    /// The words after the header, with `{1}` `{2}` `{3}` for the paths of the images,
    /// `{text}` for the text file's, `{missing}` for the one that is not there and
    /// `{spaced}` for an image in a folder with a space in its name.
    body: &'static str,
    /// How many of the images it names Claude Code makes an attachment of: those it can
    /// read, on a line of their own or at the end of one (Claude Code 2.1.308, on the Mac).
    images: usize,
}

/// The shapes: a designer's result, with the paths where a message may have them.
const SHAPES: [Shape; 12] = [
    Shape {
        name: "one path, first line of the body",
        body: "{1}\nWarm editorial desk scene.",
        images: 1,
    },
    Shape {
        name: "two paths, first lines of the body",
        body: "{1}\n{2}\nWarm editorial desk scene, and its variant.",
        images: 2,
    },
    Shape {
        name: "three paths, first lines of the body",
        body: "{1}\n{2}\n{3}\nWarm editorial desk scene, its variant and an icon.",
        images: 3,
    },
    Shape {
        name: "one path, mid body, its own line",
        body: "Saved the picture:\n{1}\nWarm editorial desk scene.",
        images: 1,
    },
    Shape {
        name: "one path inside a sentence",
        body: "Saved to {1} as asked. Warm editorial desk scene.",
        images: 0,
    },
    Shape {
        name: "two paths, mid body, text between",
        body: "Saved the pictures:\n{1}\nthe desk, and\n{2}\nthe hero.",
        images: 2,
    },
    Shape {
        name: "two paths on one line",
        body: "{1} {2}\nWarm editorial desk scene, and its variant.",
        images: 2,
    },
    Shape {
        name: "three paths, start, middle and end",
        body: "{1}\nWarm editorial desk scene.\n{2}\nThe variant.\n{3}",
        images: 3,
    },
    Shape {
        name: "a path to a file that is no image",
        body: "{text}\nNotes on the scene.",
        images: 0,
    },
    Shape {
        name: "a path to an image that is not there",
        body: "{missing}\nWarm editorial desk scene.",
        images: 0,
    },
    Shape {
        name: "a path with a space in it",
        body: "{spaced}\nWarm editorial desk scene.",
        images: 1,
    },
    Shape {
        name: "an image and a file that is no image",
        body: "{1}\n{text}\nWarm editorial desk scene, and notes.",
        images: 1,
    },
];

/// What each message asks for: a sum the message does not hold.
fn ask(sum: (u32, u32)) -> String {
    format!(
        "Reply with only the sum of {} and {}, in digits, and run no tools.",
        sum.0, sum.1
    )
}

/// Whether Claude Code makes attachments of the native paths of the machine's images where
/// it is Windows, a drive letter and backslashes (`C:\Users\…\desk.png`). It does, in each
/// of the shapes as on the Mac: the run on zeewin on 2026-10-22 (T-265), Claude Code 2.1.308,
/// whose placeholders, wrapper and companion records were those of the Mac. A run that finds
/// it does not is the answer changing, and says so in the shape's check.
const WINDOWS_CONVERTS: bool = true;

/// How many attachments `harness` makes of the paths of `shape` on this machine: Claude Code's
/// are the shape's; Devin's TUI made none of any shape (the run of 2026-10-26, T-763, Devin
/// 3000.11.205: each path stayed in the words), and a run that finds it does is the answer
/// changing.
fn expected(shape: &Shape, harness: Harness) -> usize {
    if harness != Harness::Claude || (cfg!(windows) && !WINDOWS_CONVERTS) {
        0
    } else {
        shape.images
    }
}

/// How many image placeholders (`[Image #4]`) begin `text`, as Claude Code writes them: a
/// space between two, and nothing else.
fn placeholders(text: &str) -> usize {
    let mut rest = text;
    let mut count = 0;
    while let Some(after) = rest.strip_prefix("[Image #") {
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let Some(next) = after[digits..].strip_prefix(']').filter(|_| digits > 0) else {
            break;
        };
        count += 1;
        match next.strip_prefix(' ') {
            Some(more) if more.starts_with("[Image #") => rest = more,
            _ => break,
        }
    }
    count
}

/// The header of the message a shape is, with the id `id`.
fn header(id: i64) -> String {
    format!("[ConsensFlow m-{id} · T-1 · result from @pygmalion-jolly-brook]")
}

/// The file `name` of `folder`, a `/` in `name` between folders: the path as the machine
/// writes it, a drive letter and backslashes on Windows.
fn at_path(folder: &Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}

/// The files a shape names, written into `folder`: the images are whole PNGs.
fn write_files(folder: &Path) -> Result {
    for image in IMAGES {
        files::write(&at_path(folder, image), cf_e2e::png::SMALL)?;
    }
    files::write(&at_path(folder, SPACED), cf_e2e::png::SMALL)?;
    files::write(&at_path(folder, TEXT), "Notes on the desk scene.\n")
}

/// A shape's body, with the paths filled in from `folder`.
fn body_of(shape: &Shape, folder: &Path) -> String {
    let path = |name: &str| at_path(folder, name).display().to_string();
    shape
        .body
        .replace("{1}", &path(IMAGES[0]))
        .replace("{2}", &path(IMAGES[1]))
        .replace("{3}", &path(IMAGES[2]))
        .replace("{text}", &path(TEXT))
        .replace("{missing}", &path(MISSING))
        .replace("{spaced}", &path(SPACED))
}

/// A user record of Claude's as its line says it, the bytes of an image left out.
fn shown(record: &Value) -> String {
    let mut record = record.clone();
    if let Some(parts) = record["message"]["content"].as_array_mut() {
        for part in parts {
            if part["type"] == "image" {
                part["source"]["data"] = Value::String("(bytes)".to_owned());
            }
        }
    }
    record.to_string()
}

/// The user records of the window's transcript from the `from`th line on, as
/// Claude wrote them.
fn user_records(window: &Window, from: usize) -> (usize, Vec<String>) {
    // Claude's transcript is the only record read here as the harness wrote it.
    if window.harness() != Harness::Claude {
        return (from, Vec::new());
    }
    let Some(session) = window.session() else {
        return (from, Vec::new());
    };
    let Some(file) = claude::transcript_file(&window.claude_config(), &session) else {
        return (from, Vec::new());
    };
    let text = files::read_finished_lines(&file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let records = lines
        .iter()
        .skip(from)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|record| record["type"] == "user")
        .map(|record| shown(&record))
        .collect();
    (lines.len(), records)
}

/// The user items the daemon reads, from the `from`th on.
fn users(window: &Window, from: usize) -> Vec<Item> {
    window
        .recorded()
        .into_iter()
        .filter(|item| item.role == "user")
        .skip(from)
        .collect()
}

/// Whether the engine takes the item `text` of the record for the paste of the message `id`
/// (`is_paste_of`, which the engine answers).
fn is_paste_of(text: &str, id: i64) -> bool {
    ask::engine_question(&json!({ "op": "is_paste_of", "text": text, "message": id }))
        .is_ok_and(|answer| answer == true)
}

/// Sends each shape to the window of `install`, one after another, and says what
/// the record made of it.
fn checks_of(rig: &Rig, install: &Installed, log: &Log) -> Result<Vec<Check>> {
    let name = install.harness().name();
    let window = Open::new(rig, install, "image-paste", &format!("image-paste-{name}"))
        .open_telling(log, name)?;
    if let Err(screen) = window.ready() {
        return Ok(vec![Check::failed(name, "opens", screen)]);
    }
    write_files(window.workspace())?;
    let (mut lines, mut seen_items) = (0, 0);
    let mut checks = Vec::new();
    for (at, shape) in (1_i64..).zip(&SHAPES) {
        let sum = (
            1000 + u32::try_from(at).unwrap_or(0),
            2000 + u32::try_from(at).unwrap_or(0),
        );
        let body = format!(
            "{}\n{}\n{}",
            header(at),
            body_of(shape, window.workspace()),
            ask(sum)
        );
        let body = window.given(&body)?;
        log.say(format!("{name}: sends {}", shape.name));
        let sent = window.send(&body, &(sum.0 + sum.1).to_string())?;
        window.still(ANSWER);
        let mut items = Vec::new();
        let mut shown = false;
        live::until(Duration::from_secs(10), Duration::from_millis(500), || {
            items = users(&window, seen_items);
            shown = items.iter().any(|item| is_paste_of(&item.text, at));
            shown.then_some(())
        });
        let (end, records) = user_records(&window, lines);
        (lines, seen_items) = (end, seen_items + items.len());
        let message = items
            .iter()
            .find(|item| is_paste_of(&item.text, at))
            .or(items.first());
        let attached = message.map_or(0, |item| placeholders(&item.text));
        let wanted = expected(shape, install.harness());
        log.block(
            &format!("{name}: {}", shape.name),
            &format!(
                "items the daemon reads: {:?}\nrecords: {}",
                items.iter().map(|item| &item.text).collect::<Vec<_>>(),
                records.join("\n")
            ),
        );
        let detail = format!(
            "attachments: {attached} (expected {wanted}); answered: {}; items: {}; records: {}",
            sent.shown,
            items
                .iter()
                .map(|item| format!("{:?}", cut_text(&item.text)))
                .collect::<Vec<_>>()
                .join(" | "),
            records.join(" | ")
        );
        checks.push(Check::new(
            name,
            shape.name,
            shown && attached == wanted,
            detail,
        ));
    }
    let folder = window.workspace().to_path_buf();
    window.kill();
    // What a launch's first message is, an argument of the program, is Claude Code's: Devin's
    // seeded window records no user item of it within the time given (the run of T-763, Devin
    // 3000.11.205), a question of its first message and none of its images.
    if install.harness() == Harness::Claude {
        checks.push(launch_check(rig, install, log, &folder)?);
    }
    Ok(checks)
}

/// The first message of a launch, given as an argument of the program and no paste, with the
/// path of an image on a line of its own: Claude Code 2.1.308 makes no attachment of it
/// (the probe of 2026-10-22, T-265), so the record holds the words as they were, in one item
/// and with no companion after it. A window of the folder the shapes were sent in, whose
/// files are there.
fn launch_check(rig: &Rig, install: &Installed, log: &Log, folder: &Path) -> Result<Check> {
    let name = install.harness().name();
    let what = "the first message of a launch, an argument of the program";
    let at = i64::try_from(SHAPES.len()).unwrap_or(0) + 1;
    let sum = (1000 + u32::try_from(at).unwrap_or(0), 2000);
    let picture = at_path(folder, IMAGES[0]).display().to_string();
    let body = format!(
        "{}\n{picture}\nWarm editorial desk scene.\n{}",
        header(at),
        ask(sum)
    );
    let seed = Open::new(
        rig,
        install,
        "image-paste",
        &format!("image-paste-launch-{name}"),
    );
    let window = seed.seed(&body).open_telling(log, name)?;
    let mut items = Vec::new();
    live::until(Duration::from_secs(120), Duration::from_millis(500), || {
        items = window.recorded();
        items
            .iter()
            .any(|item| item.role == "assistant" && item.complete)
            .then_some(())
    });
    log.block(
        &format!("{name}: {what}"),
        &format!(
            "items the daemon reads: {:?}",
            items.iter().map(|item| &item.text).collect::<Vec<_>>()
        ),
    );
    let users: Vec<&Item> = items.iter().filter(|item| item.role == "user").collect();
    let kept = users.len() == 1
        && placeholders(&users[0].text) == 0
        && users[0].text.contains(&picture)
        && is_paste_of(&users[0].text, at);
    window.kill();
    Ok(Check::new(
        name,
        what,
        kept,
        format!(
            "user items: {}; attachments: {}",
            users.len(),
            users.first().map_or(0, |item| placeholders(&item.text))
        ),
    ))
}

/// The first 200 characters of `text`.
fn cut_text(text: &str) -> String {
    live::cut(text, 200)
}

/// The shapes sent to one window of `harness`, and what its record made of each.
fn probe(harness: Harness) -> Result {
    let log = Log::begin(&format!("live image paste ({})", harness.name()))?;
    ask::prepare()?;
    let installs = vec![log.found(harness)?];
    let rig = Rig::start(live::rig(&installs, FAKE_AGENT))?;
    let mut checks = Vec::new();
    for install in &installs {
        checks.extend(checks_of(&rig, install, &log)?);
    }
    rig.close()?;
    report::conclude(&log, "what the record made of each shape", &checks)
}

/// A real `harness` as the chief of a project on the native daemon, sent the `case` result.
fn delivered_to(harness: Harness, case: daemon::Case) -> Result {
    let log = Log::begin(&format!("live image paste (daemon, {})", harness.name()))?;
    ask::prepare()?;
    let checks = daemon::run(&log, harness, case)?;
    report::conclude(
        &log,
        "what the board and the chief's record made of the result",
        &checks,
    )
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Claude Code; cargo xtask live image-paste (npm run live:image-paste)"]
fn claude_records_each_shape_of_a_paste_that_names_images_in_a_way_the_engine_takes_for_it(
) -> Result {
    probe(Harness::Claude)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of Devin; cargo xtask live image-paste (npm run live:image-paste)"]
fn devin_records_each_shape_of_a_paste_that_names_images_in_a_way_the_engine_takes_for_it() -> Result
{
    probe(Harness::Devin)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of Claude Code; cargo xtask live image-paste (npm run live:image-paste)"]
fn a_result_that_starts_with_an_image_path_is_delivered_to_a_claude_chief_once() -> Result {
    delivered_to(Harness::Claude, daemon::Case::Images)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of Devin; cargo xtask live image-paste (npm run live:image-paste)"]
fn a_result_that_starts_with_an_image_path_is_delivered_to_a_devin_chief_once() -> Result {
    delivered_to(Harness::Devin, daemon::Case::Images)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a chief of Claude Code; cargo xtask live image-paste (npm run live:image-paste)"]
fn a_long_result_is_delivered_to_a_claude_chief_once() -> Result {
    delivered_to(Harness::Claude, daemon::Case::Long)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_a_name_of_its_own_and_asks_for_a_sum_its_message_does_not_hold() {
        let folder = std::path::Path::new("/folder");
        let mut names: Vec<&str> = SHAPES.iter().map(|shape| shape.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SHAPES.len(), "a name twice");
        for (at, shape) in (1_u32..).zip(&SHAPES) {
            let sum = (1000 + at, 2000 + at);
            let message = format!(
                "{}\n{}\n{}",
                header(i64::from(at)),
                body_of(shape, folder),
                ask(sum)
            );
            assert!(!message.contains('{'), "a path left unfilled: {message}");
            assert!(!message.contains(&(sum.0 + sum.1).to_string()), "{message}");
            assert!(
                message.starts_with(&format!("[ConsensFlow m-{at} ")),
                "{message}"
            );
        }
    }

    #[test]
    fn the_files_the_shapes_name_are_written_whole_but_for_the_image_that_is_not_there() {
        let folder = tempfile::tempdir().unwrap();
        write_files(folder.path()).unwrap();
        for image in IMAGES.iter().chain(&[SPACED]) {
            let bytes = std::fs::read(at_path(folder.path(), image)).unwrap();
            assert!(cf_e2e::png::inspect(&bytes).is_ok(), "{image}");
        }
        assert!(at_path(folder.path(), TEXT).is_file());
        assert!(!at_path(folder.path(), MISSING).exists());
    }

    #[test]
    fn the_placeholders_that_begin_an_item_are_counted_as_claude_code_writes_them() {
        assert_eq!(placeholders("[Image #1][ConsensFlow m-1 · T-1]"), 1);
        assert_eq!(
            placeholders("[Image #2] [Image #3]\n\n<pasted_content id=\"a\">"),
            2
        );
        assert_eq!(placeholders("[Image #2][Image #3]x"), 1);
        assert_eq!(placeholders("[Image #]x"), 0);
        assert_eq!(placeholders("[ConsensFlow m-1 · T-1][Image #1]"), 0);
        assert_eq!(placeholders(""), 0);
    }

    #[test]
    fn a_shape_expects_as_many_attachments_as_it_names_readable_images_where_the_machine_converts()
    {
        let folder = std::path::Path::new("/folder");
        for shape in &SHAPES {
            let named = body_of(shape, folder).matches(".png").count();
            assert!(shape.images <= named, "{}", shape.name);
        }
        assert_eq!(
            SHAPES.iter().map(|shape| shape.images).sum::<usize>(),
            16,
            "the counts the Mac showed"
        );
        assert_eq!(
            expected(&SHAPES[2], Harness::Claude),
            if cfg!(windows) && !WINDOWS_CONVERTS {
                0
            } else {
                3
            }
        );
        // Devin's TUI makes no attachment of any shape.
        assert!(SHAPES
            .iter()
            .all(|shape| expected(shape, Harness::Devin) == 0));
    }

    #[test]
    fn the_header_of_a_shape_is_the_header_of_a_designer_s_result_in_the_field() {
        assert_eq!(
            header(2619),
            "[ConsensFlow m-2619 · T-1 · result from @pygmalion-jolly-brook]"
        );
    }
}
