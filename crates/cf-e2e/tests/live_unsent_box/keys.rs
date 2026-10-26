//! What is pressed to clear a paste left unsent in the box, and which strategies a
//! run tries and requires. A strategy is keys written into the pane as the daemon
//! writes its own (`pane.input`).

use cf_e2e::live::{ask, Harness};
use cf_e2e::{Error, Result};

/// What is pressed into the box before the next message is pasted.
#[derive(Debug)]
pub struct Strategy {
    pub name: &'static str,
    pub keys: &'static [u8],
}

/// Every strategy: nothing pressed, Control-U, Control-C and one Escape (two open
/// Claude's and Devin's rewind at an idle prompt, which takes the next Enter).
pub const STRATEGIES: [Strategy; 4] = [
    Strategy {
        name: "none",
        keys: &[],
    },
    Strategy {
        name: "ctrl-u",
        keys: &[0x15],
    },
    Strategy {
        name: "ctrl-c",
        keys: &[0x03],
    },
    Strategy {
        name: "escape",
        keys: &[0x1b],
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

/// The strategy whose keys are `keys`.
pub fn pressing(keys: &[u8]) -> Option<&'static Strategy> {
    STRATEGIES.iter().find(|strategy| strategy.keys == keys)
}

/// The strategy whose keys the app presses into `harness`'s window to clear its
/// box, as the harness code says them (`cf_harness::contract::Input`): none where
/// it presses none, or none of these.
pub fn daemons(harness: Harness) -> Result<Option<&'static Strategy>> {
    Ok(ask::clearing(harness.kind())?.and_then(|keys| pressing(&keys)))
}

/// Whether a run fails unless `strategy` cleared the box: one it is told to
/// require, or, where it is told none, the app's own key `own`.
pub fn required(own: Option<&Strategy>, strategy: &Strategy, named: &[String]) -> bool {
    if named.is_empty() {
        own.is_some_and(|own| own.name == strategy.name)
    } else {
        named.iter().any(|name| name == strategy.name)
    }
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
    fn the_four_strategies_press_their_keys_and_a_run_tries_every_one_or_those_named() {
        let keys = |name: &str| strategy(name).unwrap().keys.to_vec();
        assert_eq!(
            called(&chosen(&[]).unwrap()),
            ["none", "ctrl-u", "ctrl-c", "escape"]
        );
        assert!(keys("none").is_empty());
        assert_eq!(keys("ctrl-u"), [0x15]);
        assert_eq!(keys("ctrl-c"), [0x03]);
        assert_eq!(keys("escape"), [0x1b]);
        assert_eq!(
            called(&chosen(&names(&["escape", "none"])).unwrap()),
            ["escape", "none"]
        );
        let failed = chosen(&names(&["ctrl-x"])).unwrap_err().to_string();
        assert_eq!(failed, "no such strategy: ctrl-x");
    }

    #[test]
    fn the_apps_own_key_is_required_unless_the_run_names_what_it_requires() {
        let ctrl_c = strategy("ctrl-c").unwrap();
        let none = strategy("none").unwrap();
        assert_eq!(pressing(&[0x03]).map(|own| own.name), Some("ctrl-c"));
        assert_eq!(pressing(&[0x03, 0x03]).map(|own| own.name), None);
        assert!(required(Some(ctrl_c), ctrl_c, &[]));
        assert!(!required(Some(ctrl_c), none, &[]));
        assert!(!required(None, ctrl_c, &[]));
        // Named, it is what is required.
        assert!(required(Some(ctrl_c), none, &names(&["none"])));
        assert!(!required(Some(ctrl_c), ctrl_c, &names(&["none"])));
    }
}
