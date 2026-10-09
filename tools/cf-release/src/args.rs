//! The words after a step's name. Every flag a step takes is `--name` and the
//! one word after it, read as `prepare-update.mjs` (the script `prepare-update`
//! replaced) read them and refused in its words: a word that is no flag of the
//! step, a flag with no value (nothing after it, or another flag), a flag given
//! twice, a flag the step needs and was not given.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::cli::Failure;

/// What a step was given, by flag name without its dashes.
pub struct Flags {
    values: BTreeMap<&'static str, OsString>,
}

impl Flags {
    /// Reads `args` as `--name value` pairs, each `name` one of `accepted`.
    pub fn read(args: &[OsString], accepted: &[&'static str]) -> Result<Self, Failure> {
        let mut values = BTreeMap::new();
        let mut words = args.iter();
        while let Some(word) = words.next() {
            let text = word.to_string_lossy();
            let name = text
                .strip_prefix("--")
                .and_then(|name| accepted.iter().copied().find(|known| *known == name));
            let Some(name) = name else {
                return Err(Failure::Usage(format!("unknown argument: {text}")));
            };
            let value = words
                .next()
                .filter(|value| !value.as_encoded_bytes().starts_with(b"--"));
            let Some(value) = value else {
                return Err(Failure::Usage(format!("--{name} needs a value")));
            };
            if values.insert(name, value.clone()).is_some() {
                return Err(Failure::Usage(format!("duplicate argument: --{name}")));
            }
        }
        Ok(Self { values })
    }

    /// What `--name` was given, if it was.
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.values.get(name).map(OsString::as_os_str)
    }

    /// What `--name` was given, or the refusal that it was not.
    pub fn require(&self, name: &str) -> Result<&OsStr, Failure> {
        self.get(name)
            .ok_or_else(|| Failure::Usage(format!("missing required argument: --{name}")))
    }

    /// The checkout `--repo` names, or the folder this is run in, as a whole path.
    pub fn repo(&self) -> Result<PathBuf, Failure> {
        let repo = match self.get("repo") {
            Some(repo) => PathBuf::from(repo),
            None => std::env::current_dir().map_err(|cause| {
                Failure::Failed(format!("could not tell which folder this is: {cause}"))
            })?,
        };
        std::path::absolute(&repo).map_err(|cause| {
            Failure::Failed(format!(
                "could not tell where {} is: {cause}",
                repo.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<OsString> {
        line.split_whitespace().map(OsString::from).collect()
    }

    fn refusal(line: &str) -> String {
        let accepted = ["bundle", "date", "repo"];
        match Flags::read(&words(line), &accepted) {
            Err(Failure::Usage(said)) => said,
            Err(other) => panic!("{line}: {other:?}"),
            Ok(_) => panic!("{line}: was read"),
        }
    }

    #[test]
    fn each_flag_takes_the_word_after_it() {
        let flags = Flags::read(
            &words("--date 2026-09-09T12:00:00Z --bundle a.app"),
            &["bundle", "date", "repo"],
        )
        .unwrap();
        assert_eq!(flags.get("bundle"), Some(OsStr::new("a.app")));
        assert_eq!(flags.get("date"), Some(OsStr::new("2026-09-09T12:00:00Z")));
        assert_eq!(flags.get("repo"), None);
        assert!(Flags::read(&[], &["repo"]).unwrap().get("repo").is_none());
    }

    #[test]
    fn what_is_not_a_flag_of_the_step_is_unknown() {
        assert_eq!(refusal("--url x"), "unknown argument: --url");
        assert_eq!(refusal("x"), "unknown argument: x");
        assert_eq!(refusal("-x"), "unknown argument: -x");
        // Only `--name value`, not `--name=value`.
        assert_eq!(refusal("--date=x"), "unknown argument: --date=x");
        // It is found where it stands, before anything wrong after it.
        assert_eq!(
            refusal("--date 1 --url --bundle"),
            "unknown argument: --url"
        );
    }

    #[test]
    fn a_flag_without_a_value_or_given_twice_is_refused() {
        assert_eq!(refusal("--date"), "--date needs a value");
        assert_eq!(refusal("--date --bundle x"), "--date needs a value");
        assert_eq!(refusal("--date 1 --date 2"), "duplicate argument: --date");
        // A value is any word that is not a flag, a lone dash included.
        let flags = Flags::read(&words("--date -"), &["date"]).unwrap();
        assert_eq!(flags.get("date"), Some(OsStr::new("-")));
    }

    #[test]
    fn a_flag_the_step_needs_is_asked_for_by_name() {
        let flags = Flags::read(&words("--bundle x"), &["bundle", "date"]).unwrap();
        assert_eq!(flags.require("bundle").unwrap(), OsStr::new("x"));
        let Err(Failure::Usage(said)) = flags.require("date") else {
            panic!("a flag that was not given was required");
        };
        assert_eq!(said, "missing required argument: --date");
    }

    #[test]
    fn the_checkout_is_the_one_named_or_the_folder_it_is_run_in() {
        let named = Flags::read(&words("--repo /some/where"), &["repo"]).unwrap();
        assert_eq!(
            named.repo().unwrap(),
            std::path::absolute("/some/where").unwrap()
        );
        let unnamed = Flags::read(&[], &["repo"]).unwrap();
        assert_eq!(unnamed.repo().unwrap(), std::env::current_dir().unwrap());
        // A relative one is made whole, as `path.resolve` made it.
        let relative = Flags::read(&words("--repo here"), &["repo"]).unwrap();
        assert_eq!(
            relative.repo().unwrap(),
            std::env::current_dir().unwrap().join("here")
        );
    }
}
