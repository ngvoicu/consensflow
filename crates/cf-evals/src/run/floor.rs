//! Where a run speaks and what it keeps of what it said: a line for each thing it
//! does, with the seconds since it began, on the console as it goes and in the
//! report after; and the wait for a window to have drawn its prompt, which a run
//! makes before it types to it.

use std::fmt::Display;
use std::io::Write;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::Result;

/// What a run passes each line through before it says or keeps it.
type Masking = Box<dyn Fn(&str) -> String>;

/// A run that speaks, and keeps what it said.
pub struct Floor<'o> {
    out: &'o mut dyn Write,
    started: Instant,
    log: Vec<(u64, String)>,
    mask: Option<Masking>,
}

impl<'o> Floor<'o> {
    /// A run beginning now, which speaks on `out`.
    pub fn new(out: &'o mut dyn Write) -> Self {
        Self {
            out,
            started: Instant::now(),
            log: Vec::new(),
            mask: None,
        }
    }

    /// From now on, every thing the run notes is passed through `mask` before it is
    /// said or kept: a run that hides what looks like a key or a token in what it says
    /// as it goes (the long run's), whatever says it (a wait that gave up and shows a
    /// window's screen among them). What it says as it is ([`Floor::say`]: where its
    /// report is, the report masked already) is its own.
    pub fn masking(&mut self, mask: impl Fn(&str) -> String + 'static) {
        self.mask = Some(Box::new(mask));
    }

    /// `line` as it is noted: through the run's mask, where it has one.
    fn shown(&self, line: impl Display) -> String {
        let line = line.to_string();
        match &self.mask {
            Some(mask) => mask(&line),
            None => line,
        }
    }

    /// How long ago the run began.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// How many seconds ago it began, to the nearest.
    pub fn seconds(&self) -> u64 {
        self.started.elapsed().as_secs_f64().round() as u64
    }

    /// Says `line` as a thing the run did, with the seconds since it began, and
    /// keeps it for the report: `  [12s] chief idle; tasks T-1:working`.
    pub fn note(&mut self, line: impl Display) -> Result {
        let (at, line) = (self.seconds(), self.shown(line));
        writeln!(self.out, "  [{at}s] {line}")?;
        self.log.push((at, line));
        Ok(())
    }

    /// Says `text` as it is, a line.
    pub fn say(&mut self, text: impl Display) -> Result {
        Ok(writeln!(self.out, "{text}")?)
    }

    /// What the run did, as the report keeps it: each line with its second.
    pub fn log(&self) -> Value {
        Value::Array(
            self.log
                .iter()
                .map(|(at, line)| json!({ "at": at, "line": line }))
                .collect(),
        )
    }
}

/// How often a window's output is looked at, how long it holds still before it
/// counts as drawn, and how long it is waited for.
const LOOK: Duration = Duration::from_millis(500);
const STILL: Duration = Duration::from_secs(3);
const CAP: Duration = Duration::from_secs(60);

/// Waits until a window's output (`size`, its length so far) has grown and then
/// held still for three seconds: the harness has drawn its prompt. A window can read
/// idle the moment it opens (Pi and Devin do), and text typed before the prompt is
/// lost. It waits a minute at most.
pub fn settled(size: impl FnMut() -> usize) {
    settled_within(size, LOOK, STILL, CAP);
}

/// [`settled`], looking every `look`, holding still for `still`, for `cap` at most.
fn settled_within(mut size: impl FnMut() -> usize, look: Duration, still: Duration, cap: Duration) {
    let started = Instant::now();
    let mut last = size();
    let mut since = started;
    loop {
        sleep(look);
        let now = size();
        if now != last {
            last = now;
            since = Instant::now();
        } else if now > 0 && since.elapsed() >= still {
            return;
        }
        if started.elapsed() > cap {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_thing_the_run_did_is_said_with_its_second_and_kept_for_the_report() {
        let mut said = Vec::new();
        let mut floor = Floor::new(&mut said);
        floor
            .note("chief (claude) ready; typing the prompt")
            .unwrap();
        floor.say("daemon: native (rust 3.0.1)").unwrap();
        floor.note("quiet: the run is over").unwrap();
        assert_eq!(
            floor.log(),
            json!([
                { "at": 0, "line": "chief (claude) ready; typing the prompt" },
                { "at": 0, "line": "quiet: the run is over" },
            ])
        );
        assert!(floor.elapsed() < Duration::from_secs(5));
        assert_eq!(floor.seconds(), 0);
        let said = String::from_utf8(said).unwrap();
        assert_eq!(
            said,
            "  [0s] chief (claude) ready; typing the prompt\n\
             daemon: native (rust 3.0.1)\n\
             \x20 [0s] quiet: the run is over\n"
        );
    }

    #[test]
    fn a_run_that_masks_says_and_keeps_each_thing_it_notes_through_its_mask() {
        let mut said = Vec::new();
        let mut floor = Floor::new(&mut said);
        floor.masking(|line: &str| line.replace("hunter2", "[masked]"));
        floor
            .note("gave up waiting: its screen: password hunter2")
            .unwrap();
        // What it says as it is (where its report is) is its own.
        floor.say("report: /tmp/kept-hunter2.json").unwrap();
        assert_eq!(
            floor.log(),
            json!([{ "at": 0, "line": "gave up waiting: its screen: password [masked]" }])
        );
        let said = String::from_utf8(said).unwrap();
        assert_eq!(
            said,
            "  [0s] gave up waiting: its screen: password [masked]\nreport: /tmp/kept-hunter2.json\n"
        );
    }

    #[test]
    fn a_window_has_drawn_its_prompt_once_its_output_grew_and_then_held_still() {
        // Grows twice, then holds: settled when it has held for the time asked.
        let sizes = Cell::new(0_usize);
        let looks = Cell::new(0_u32);
        let started = Instant::now();
        settled_within(
            || {
                looks.set(looks.get() + 1);
                if looks.get() < 4 {
                    sizes.set(sizes.get() + 10);
                }
                sizes.get()
            },
            Duration::from_millis(5),
            Duration::from_millis(30),
            Duration::from_secs(5),
        );
        // It looked again once the output held (the fourth look is the first that
        // finds it unchanged), and waited the hold out: how many looks that took is
        // the machine's, and one slow sleep can outlast the whole hold.
        assert!(
            looks.get() >= 4,
            "it looked after the output held: {}",
            looks.get()
        );
        assert!(started.elapsed() >= Duration::from_millis(30));
        assert_eq!(sizes.get(), 30);
    }

    #[test]
    fn a_window_that_prints_nothing_is_waited_for_no_longer_than_the_cap_and_one_that_never_stops_too(
    ) {
        let started = Instant::now();
        settled_within(
            || 0,
            Duration::from_millis(5),
            Duration::from_millis(10),
            Duration::from_millis(60),
        );
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(60) && waited < Duration::from_secs(2),
            "{waited:?}"
        );
        let grown = Cell::new(0_usize);
        let started = Instant::now();
        settled_within(
            || {
                grown.set(grown.get() + 1);
                grown.get()
            },
            Duration::from_millis(5),
            Duration::from_millis(20),
            Duration::from_millis(60),
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
