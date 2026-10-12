//! What is pressed to clear the box, and which strategies a run tries and
//! requires. A strategy is keys written into the pane as the daemon writes its
//! own, and the pauses between them.

use std::time::Duration;

use cf_e2e::{Error, Result};

/// The key that interrupts a turn, as the daemon presses it.
pub const ESCAPE: u8 = 0x1b;

/// The strategy whose keys the daemon presses (`CLEAR_INPUT`, `crates/cf-harness/src/claude/stopped.rs`):
/// the one a run requires where it is tried, unless it is told otherwise.
pub const DAEMONS: &str = "ctrl-c";

/// A step of a strategy: keys pressed into the window, or a pause between them.
#[derive(Debug)]
pub enum Step {
    Keys(&'static [u8]),
    Pause(Duration),
}

/// What is pressed, once Claude reads idle, to clear what it put back into its box.
#[derive(Debug)]
pub struct Strategy {
    pub name: &'static str,
    pub steps: &'static [Step],
}

/// Every strategy: nothing pressed, Control-U, Control-C and two Escapes.
pub const STRATEGIES: [Strategy; 4] = [
    Strategy {
        name: "none",
        steps: &[],
    },
    Strategy {
        name: "ctrl-u",
        steps: &[Step::Keys(&[0x15])],
    },
    Strategy {
        name: DAEMONS,
        steps: &[Step::Keys(&[0x03])],
    },
    Strategy {
        name: "esc-esc",
        steps: &[
            Step::Keys(&[ESCAPE]),
            Step::Pause(Duration::from_millis(300)),
            Step::Keys(&[ESCAPE]),
        ],
    },
];

/// The strategy called `name`.
pub fn strategy(name: &str) -> Result<&'static Strategy> {
    STRATEGIES
        .iter()
        .find(|strategy| strategy.name == name)
        .ok_or_else(|| Error::Daemon(format!("no such strategy: {name}")))
}

/// The strategies a run is asked to try: those named, or every one.
pub fn chosen(names: &[String]) -> Result<Vec<&'static Strategy>> {
    if names.is_empty() {
        return Ok(STRATEGIES.iter().collect());
    }
    names.iter().map(|name| strategy(name)).collect()
}

/// The strategies a run fails without: those it is told to require, or, if it is
/// told none, the daemon's own key where it is among those tried.
pub fn required(names: &[String], chosen: &[&'static Strategy]) -> Result<Vec<&'static Strategy>> {
    if names.is_empty() {
        return Ok(chosen
            .iter()
            .copied()
            .filter(|strategy| strategy.name == DAEMONS)
            .collect());
    }
    names.iter().map(|name| strategy(name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    fn called(strategies: &[&Strategy]) -> Vec<&'static str> {
        strategies.iter().map(|strategy| strategy.name).collect()
    }

    #[test]
    fn the_four_strategies_press_what_the_findings_say_of_each() {
        let keys = |name: &str| -> Vec<String> {
            strategy(name)
                .unwrap()
                .steps
                .iter()
                .map(|step| match step {
                    Step::Keys(keys) => format!("{keys:?}"),
                    Step::Pause(pause) => format!("{} ms", pause.as_millis()),
                })
                .collect()
        };
        assert_eq!(
            called(&STRATEGIES.iter().collect::<Vec<_>>()),
            ["none", "ctrl-u", "ctrl-c", "esc-esc"]
        );
        // What a paste does with nothing pressed first.
        assert!(keys("none").is_empty());
        // The line the cursor is on, which leaves the first line of a two-line message.
        assert_eq!(keys("ctrl-u"), ["[21]"]);
        // Control-C, the key the daemon presses to clear a box.
        assert_eq!(keys("ctrl-c"), ["[3]"]);
        // Two Escapes with a pause between them: the second opens the rewind at an empty box.
        assert_eq!(keys("esc-esc"), ["[27]", "300 ms", "[27]"]);
        assert_eq!(DAEMONS, "ctrl-c");
    }

    #[test]
    fn a_run_tries_every_strategy_or_those_named_and_refuses_a_name_it_does_not_have() {
        assert_eq!(
            called(&chosen(&[]).unwrap()),
            ["none", "ctrl-u", "ctrl-c", "esc-esc"]
        );
        // In the order named, as many times as named.
        assert_eq!(
            called(&chosen(&names(&["esc-esc", "none", "esc-esc"])).unwrap()),
            ["esc-esc", "none", "esc-esc"]
        );
        let failed = chosen(&names(&["ctrl-c", "ctrl-x"]))
            .unwrap_err()
            .to_string();
        assert_eq!(failed, "no such strategy: ctrl-x");
        let failed = required(&names(&["nothing"]), &[]).unwrap_err().to_string();
        assert_eq!(failed, "no such strategy: nothing");
    }

    #[test]
    fn the_daemons_own_key_is_required_where_it_is_tried_unless_the_run_names_what_it_requires() {
        let all = chosen(&[]).unwrap();
        assert_eq!(called(&required(&[], &all).unwrap()), ["ctrl-c"]);
        // Not tried, nothing is required: a run of `none` alone asks for no strategy.
        let none = chosen(&names(&["none"])).unwrap();
        assert!(required(&[], &none).unwrap().is_empty());
        // Named, it is what is required, whatever is tried.
        assert_eq!(
            called(&required(&names(&["esc-esc"]), &all).unwrap()),
            ["esc-esc"]
        );
        assert_eq!(
            called(&required(&names(&["ctrl-c"]), &none).unwrap()),
            ["ctrl-c"]
        );
    }
}
