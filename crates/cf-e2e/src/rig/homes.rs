//! The homes of the real harnesses that a rig's windows run on in place of the
//! owner's: what the rig asks of each when it starts (the variables its windows
//! are given) and when it ends (what the run came to).
//!
//! A rig clears the environment its daemon, pane host and windows have, and sets
//! its own. A login the owner keeps in a folder of their own is reached by a
//! variable the rig sets (or does not set: Pi reads `~/.pi/agent` when
//! `PI_CODING_AGENT_DIR` names nothing), so what a window of a real harness did
//! to it was the owner's, and a Pi that refreshed its token rotated theirs
//! (T-696). A [`HarnessHome`] is the other way: a folder of the run's own that
//! holds a copy of the login, whose variables the rig sets **last**, over what
//! the rig set and over every [`Config::var`](super::Config::var) of the case,
//! and that is asked what the run came to when the windows are gone.
//!
//! Claude Code's config folder ([`ClaudeConfig`](super::ClaudeConfig)) is one: it
//! is held to the same start and end here, so the rig has one path for them all.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{ClaudeConfig, Config};
use crate::{Error, Result};

/// A home of a real harness's own that a rig's windows run on in place of the
/// owner's ([`Config::isolated`]).
pub trait HarnessHome: std::fmt::Debug + Send + Sync {
    /// The variables its windows are given, made when first asked for and the
    /// same after: a rig that starts again on the same home (the app's restart)
    /// finds what the run kept there. An error says why there is no home, said
    /// before anything is spent.
    fn vars(&self) -> Result<Vec<(String, String)>>;

    /// Where the login in the home is copied from, as a run says it.
    fn described(&self) -> String;

    /// Copies what the run's harness kept in the home that a kept home keeps
    /// (never a login) into `into`, the `.claude` of the home the rig made: asked
    /// when a rig whose home is kept ends, the windows gone, before
    /// [`HarnessHome::finish`].
    fn keep_records(&self, _into: &Path) -> Result {
        Ok(())
    }

    /// What the run came to in the home and in the owner's setup, asked when a
    /// rig that used the home ends, the windows gone: an error says what the run
    /// did that it was not to, and fails the case that ends the rig.
    fn finish(&self) -> Result;
}

/// Claude Code's config folder, as a home: the one variable it is.
#[derive(Debug)]
struct ClaudeHome(Arc<dyn ClaudeConfig>);

impl HarnessHome for ClaudeHome {
    fn vars(&self) -> Result<Vec<(String, String)>> {
        let folder: PathBuf = self.0.folder()?;
        Ok(vec![(
            "CLAUDE_CONFIG_DIR".to_owned(),
            folder.to_string_lossy().into_owned(),
        )])
    }

    fn described(&self) -> String {
        self.0.described()
    }

    fn keep_records(&self, into: &Path) -> Result {
        self.0.keep_records(into)
    }

    fn finish(&self) -> Result {
        self.0.finish()
    }
}

/// The homes a rig runs on, from the moment it asked them for their variables to
/// the moment it asked what the run came to.
pub(super) struct Homes {
    list: Vec<Arc<dyn HarnessHome>>,
    finished: bool,
}

impl Homes {
    /// The homes of `config`, asked for their variables in the order they were
    /// given (Claude Code's first): the first that cannot be made is why the rig
    /// does not start, before anything else is spent.
    pub(super) fn start(config: &Config) -> Result<(Self, Vec<(String, String)>)> {
        let mut list: Vec<Arc<dyn HarnessHome>> = Vec::new();
        if let Some(claude) = &config.claude_config {
            list.push(Arc::new(ClaudeHome(Arc::clone(claude))));
        }
        for home in &config.isolated {
            if !list.iter().any(|held| Arc::ptr_eq(held, home)) {
                list.push(Arc::clone(home));
            }
        }
        let mut vars = Vec::new();
        let mut each = Vec::new();
        for home in &list {
            let own = home.vars()?;
            each.push((home.described(), own.clone()));
            vars.extend(own);
        }
        if let Some(said) = clashing(&each, cfg!(windows)) {
            return Err(Error::Daemon(said));
        }
        Ok((
            Self {
                list,
                finished: false,
            },
            vars,
        ))
    }

    /// Asks each home what the run came to, once: the answer to the case that
    /// ends the rig. `kept` is where a kept home keeps what the harnesses left.
    /// Every home is asked; the errors are said together.
    pub(super) fn finish(&mut self, kept: Option<&Path>) -> Result {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        let mut said = Vec::new();
        for home in &self.list {
            let kept = kept.map_or(Ok(()), |into| home.keep_records(into));
            if let Err(failed) = home.finish().and(kept) {
                said.push(failed);
            }
        }
        match said.len() {
            0 => Ok(()),
            1 => Err(said.remove(0)),
            _ => Err(Error::Daemon(
                said.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )),
        }
    }
}

/// The variable two homes set to different values, in words: the rig sets its homes' variables
/// in turn over each other's, so the later would silently take the earlier's away (T-1136: Devin
/// on Windows moves `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` for the whole rig, and no other
/// harness's home may name them). `each` is every home, as it describes where its login is copied
/// from, with the variables it sets; Windows reads a variable's name in either case
/// (`case_insensitive`). None where no two homes disagree.
pub(crate) fn clashing(
    each: &[(String, Vec<(String, String)>)],
    case_insensitive: bool,
) -> Option<String> {
    let key = |name: &str| {
        if case_insensitive {
            name.to_ascii_uppercase()
        } else {
            name.to_owned()
        }
    };
    let mut set: Vec<(String, &str, &str)> = Vec::new();
    for (from, vars) in each {
        for (name, value) in vars {
            if let Some((_, earlier, was)) = set
                .iter()
                .find(|(held, _, was)| *held == key(name) && was != value)
            {
                return Some(format!(
                    "{earlier}'s home and {from}'s both set {name}, to {was} and {value}: the second would take the first's away. A harness's home sets the variables of that harness alone"
                ));
            }
            set.push((key(name), from, value));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{clashing, HarnessHome, Homes};
    use crate::rig::Config;
    use crate::Result;

    fn home(from: &str, vars: &[(&str, &str)]) -> (String, Vec<(String, String)>) {
        (
            from.to_owned(),
            vars.iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }

    /// Two homes that set one variable to two values: the rig refuses, naming the variable
    /// and both homes, rather than let the second take the first's away.
    #[test]
    fn two_homes_that_set_one_variable_to_two_values_are_a_refusal_naming_both() {
        let homes = [
            home("Devin's login", &[("USERPROFILE", "/run/profile")]),
            home(
                "Pi's login",
                &[
                    ("PI_CODING_AGENT_DIR", "/run/pi"),
                    ("USERPROFILE", "/run/other"),
                ],
            ),
        ];
        let said = clashing(&homes, false).expect("a clash");
        assert!(said.contains("USERPROFILE"), "{said}");
        assert!(
            said.contains("Devin's login") && said.contains("Pi's login"),
            "{said}"
        );
        assert!(
            said.contains("/run/profile") && said.contains("/run/other"),
            "{said}"
        );
    }

    /// The same value twice is no clash (a home several harnesses share), and homes that set
    /// different variables are none.
    #[test]
    fn the_same_value_twice_and_different_variables_are_no_clash() {
        let same = [
            home("A", &[("XDG_DATA_HOME", "/run/data")]),
            home(
                "B",
                &[("XDG_DATA_HOME", "/run/data"), ("CODEX_HOME", "/run/codex")],
            ),
        ];
        assert_eq!(clashing(&same, false), None);
        assert_eq!(clashing(&[], false), None);
    }

    /// Windows reads `Path` and `PATH` as one variable; elsewhere they are two.
    #[test]
    fn windows_reads_a_variable_in_either_case_and_a_mac_does_not() {
        let homes = [
            home("A", &[("AppData", "/one")]),
            home("B", &[("APPDATA", "/two")]),
        ];
        assert!(clashing(&homes, true).is_some());
        assert_eq!(clashing(&homes, false), None);
    }

    /// A home that sets what it is told to.
    #[derive(Debug)]
    struct Told(&'static str, Vec<(&'static str, &'static str)>);

    impl HarnessHome for Told {
        fn vars(&self) -> Result<Vec<(String, String)>> {
            Ok(self
                .1
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect())
        }

        fn described(&self) -> String {
            self.0.to_owned()
        }

        fn finish(&self) -> Result {
            Ok(())
        }
    }

    /// The rig asks its homes for their variables when it starts, and a pair that clash is
    /// the reason it does not: before a window opens, nothing spent.
    #[test]
    fn a_rig_whose_homes_clash_does_not_start_and_one_whose_homes_do_not_does() {
        let clashing = Config::new("fake-agent")
            .isolated(Arc::new(Told("Devin's", vec![("USERPROFILE", "/a")])))
            .isolated(Arc::new(Told("Pi's", vec![("USERPROFILE", "/b")])));
        let refused = Homes::start(&clashing)
            .err()
            .expect("a refusal")
            .to_string();
        assert!(
            refused.contains("Devin's") && refused.contains("Pi's"),
            "{refused}"
        );
        let apart = Config::new("fake-agent")
            .isolated(Arc::new(Told("Devin's", vec![("USERPROFILE", "/a")])))
            .isolated(Arc::new(Told("Pi's", vec![("PI_CODING_AGENT_DIR", "/b")])));
        let Ok((_, vars)) = Homes::start(&apart) else {
            panic!("homes that set different variables start");
        };
        assert_eq!(vars.len(), 2);
    }
}
