//! The chief's window of a project, as the owner meets it: what the board says it is
//! doing, what is on its screen, and what the owner types into it. The owner types
//! as the human does (a bracketed paste, then Enter, once the chief is idle) and
//! presses Enter again while the window keeps the text.

use std::sync::OnceLock;
use std::thread::sleep;
use std::time::Duration;

use cf_e2e::live::text;
use cf_e2e::live::trace::Trace;
use cf_e2e::pattern;
use cf_e2e::rig::{Project, Rig};
use regex::Regex;
use serde_json::{json, Value};

use super::floor::{settled, Floor};
use crate::plan::last_lines;
use crate::Result;

/// How long a chief is waited for to be idle before the owner types to it, and how
/// long its window may take to come up.
pub const READY: Duration = Duration::from_secs(240);

/// How long a chief is given to go to work on what was typed to it before the owner
/// presses Enter again, and how many times at most: Devin takes a pasted prompt into
/// its box and waits for an Enter of its own; Codex on Windows once took the second
/// Enter as a new line too. A window that went to work took the text, however soon
/// it was done.
pub const TAKES: Duration = Duration::from_secs(5);
const ENTERS: usize = 3;

/// The key Enter sends.
const ENTER: u8 = 13;

/// What a chief's window is doing that is neither of `idle`: the board's word for it.
pub const IDLE: &str = "idle";

/// The id of the window opened for a chief: `p1-chief-…`, or the last `-chief`.
fn is_a_chief(id: &str) -> bool {
    static CHIEF: OnceLock<Regex> = OnceLock::new();
    pattern::once(&CHIEF, r"^p\d+-chief-|-chief$").is_match(id)
}

/// The chief of a project of a rig.
pub struct Chief<'r> {
    rig: &'r Rig,
    project: Project<'r>,
}

impl<'r> Chief<'r> {
    pub fn new(rig: &'r Rig, project: Project<'r>) -> Self {
        Self { rig, project }
    }

    /// The project's board, as the page draws it.
    pub fn board(&self) -> Result<Value> {
        Ok(self.project.board()?)
    }

    /// The chief's lane of a board.
    pub fn lane_of(board: &Value) -> Value {
        board["lanes"]
            .as_array()
            .and_then(|lanes| {
                lanes
                    .iter()
                    .find(|lane| lane["participant"]["handle"] == "chief")
            })
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// The chief's lane now.
    pub fn lane(&self) -> Result<Value> {
        Ok(Self::lane_of(&self.board()?))
    }

    /// What the chief's window is doing, as the board says: `idle`, `working`,
    /// `waiting` (on a dialog of its own); none where the board says nothing.
    pub fn state(&self) -> Result<Option<String>> {
        Ok(self.lane()?["activity"]["state"]
            .as_str()
            .map(str::to_owned))
    }

    /// The chief's window: its id and its generation.
    pub fn pane(&self) -> Result<Value> {
        Ok(self.lane()?["pane"].clone())
    }

    /// Everything the window `pane` printed, as the text on its screen.
    pub fn screen(&self, pane: &Value) -> String {
        self.rig.screen(pane["id"].as_str().unwrap_or_default())
    }

    /// How much the window `pane` printed so far.
    pub fn printed(&self, pane: &Value) -> usize {
        self.screen(pane).len()
    }

    /// Waits until the window `pane` has drawn its prompt and held still.
    pub fn settles(&self, pane: &Value) {
        settled(|| self.printed(pane));
    }

    /// The last `count` lines the window `pane` printed.
    pub fn last_lines(&self, pane: &Value, count: usize) -> Vec<String> {
        last_lines(&self.screen(pane), count)
    }

    /// Presses the keys `bytes` in the window `pane`, as the app does.
    pub fn press(&self, pane: &Value, bytes: &[u8]) -> Result {
        self.rig.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": pane["generation"], "bytes": bytes }),
        )?;
        Ok(())
    }

    /// A wait that, when it gives up, says what the chief's window was doing and
    /// showed.
    pub fn waits_for(
        &self,
        floor: &mut Floor,
        what: &str,
        within: Duration,
        mut predicate: impl FnMut() -> Result<bool>,
    ) -> Result {
        let waited = self.rig.wait_for(what, within, || {
            predicate().map_err(|failed| cf_e2e::Error::Daemon(failed.to_string()))
        });
        if let Err(cause) = waited {
            self.gave_up(floor, what)?;
            return Err(cause.into());
        }
        Ok(())
    }

    /// Says why a wait for `what` was given up: the chief's activity, how its window
    /// was started, and the end of its screen.
    fn gave_up(&self, floor: &mut Floor, what: &str) -> Result {
        let lane = self.lane().unwrap_or(Value::Null);
        // A window that closed already: the last one opened for the chief, as it was started.
        let opened = self
            .rig
            .open_frames()
            .into_iter()
            .rfind(|frame| is_a_chief(frame["id"].as_str().unwrap_or_default()));
        let pane = if lane["pane"].is_null() {
            opened.clone().unwrap_or(Value::Null)
        } else {
            lane["pane"].clone()
        };
        // The screen's rows through the run's screen mask first, as one text, and only
        // then its last 12 taken: a secret's name may end one row and its value begin
        // the next.
        let shown = if pane.is_null() {
            Vec::new()
        } else {
            let lines = floor.screen(&self.last_lines(&pane, 40));
            lines[lines.len().saturating_sub(12)..].to_vec()
        };
        let started = opened.map_or_else(String::new, |frame| {
            let argv: Vec<String> = frame["argv"]
                .as_array()
                .map(|argv| argv.iter().map(text).collect())
                .unwrap_or_default();
            started_as(&argv, floor)
        });
        let shown = shown.join(" ⏎ ");
        floor.note(format!(
            "gave up waiting for {what}: chief {};{started} its screen: {shown}",
            serde_json::to_string(&lane["activity"]).unwrap_or_else(|_| "null".to_owned()),
        ))
    }

    /// Types into the chief's terminal as the owner would, Enter pressed again while
    /// the window keeps the text.
    pub fn says(&self, floor: &mut Floor, words: &str) -> Result {
        self.waits_for(floor, "the chief to be idle", READY, || {
            Ok(self.state()?.as_deref() == Some(IDLE))
        })?;
        let mark = Trace::read(self.rig).len();
        self.rig.tell_within(self.project.id(), words, READY)?;
        for _ in 0..ENTERS {
            sleep(TAKES);
            if Trace::read(self.rig).worked_after("chief", mark)
                || self.state()?.as_deref() != Some(IDLE)
            {
                return Ok(());
            }
            let pane = self.pane()?;
            self.press(&pane, &[ENTER])?;
            floor.note("Enter pressed again: the window had not taken the text")?;
        }
        Ok(())
    }
}

/// What the chief's window was started as, `argv`, as a wait that gave up says it: how
/// long it was, and each word, one of more than 100 characters cut to them and said how
/// long it was. Each word through the run's mask whole before it is cut: a secret cut
/// short is no secret's shape, and would be kept in part.
fn started_as(argv: &[String], floor: &Floor) -> String {
    let whole: usize = argv.join(" ").chars().count();
    let words: Vec<String> = argv
        .iter()
        .map(|word| {
            let word = floor.shown(word);
            if word.chars().count() > 100 {
                format!("{}…({})", cut_to(&word, 100), word.chars().count())
            } else {
                word
            }
        })
        .collect();
    format!(" started ({whole} characters) as {};", words.join(" "))
}

/// The first `most` characters of `word`.
fn cut_to(word: &str, most: usize) -> String {
    word.chars().take(most).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_a_chiefs_by_the_id_the_daemon_gave_it() {
        for id in ["p1-chief-1", "p12-chief-claude", "x-chief", "chief"] {
            assert_eq!(is_a_chief(id), id != "chief", "{id}");
        }
        assert!(!is_a_chief("p1-worker-2"));
        assert!(!is_a_chief("trust-1"));
        assert!(!is_a_chief("chiefly"));
    }

    #[test]
    fn the_chiefs_lane_is_the_one_of_its_handle_and_none_where_the_board_has_none() {
        let board = json!({ "lanes": [
            { "participant": { "handle": "zeus" }, "activity": { "state": "working" } },
            { "participant": { "handle": "chief" }, "activity": { "state": "idle" }, "pane": { "id": "p1-chief-1", "generation": 1 } },
        ] });
        assert_eq!(Chief::lane_of(&board)["pane"]["id"], "p1-chief-1");
        assert_eq!(Chief::lane_of(&json!({ "lanes": [] })), Value::Null);
        assert_eq!(Chief::lane_of(&Value::Null), Value::Null);
    }

    #[test]
    fn each_word_of_a_command_line_is_masked_whole_before_it_is_cut() {
        // Astra's: a word of 84 characters and a token after them, which the cut at 100
        // would leave 16 characters of: no token's shape, kept in part.
        let word = format!(
            "{}ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "x ".repeat(42)
        );
        let mut out = Vec::new();
        let floor = Floor::masked(
            &mut out,
            |line| crate::long_run::mask::mask(line, &crate::long_run::mask::Names::default()),
            <[String]>::to_vec,
        );
        let said = started_as(&["claude".to_owned(), word], &floor);
        assert!(!said.contains("ghp_abcdef"), "{said}");
        assert!(said.contains("[masked]"), "{said}");
        // Without a mask, as it was, cut where it says how long it was.
        let mut out = Vec::new();
        let plain = Floor::new(&mut out);
        let said = started_as(&["a".repeat(101)], &plain);
        assert_eq!(
            said,
            format!(" started (101 characters) as {}…(101);", "a".repeat(100))
        );
    }

    #[test]
    fn a_long_word_of_a_command_line_is_cut_where_it_says_how_long_it_was() {
        assert_eq!(cut_to("abcdef", 3), "abc");
        assert_eq!(cut_to("ab", 3), "ab");
    }
}
