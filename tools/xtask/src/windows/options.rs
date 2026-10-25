//! The words of `windows` and `windows-matrix`, read as the JavaScript tools read
//! theirs (Node's `parseArgs`): long options only, `--name value` or
//! `--name=value`, and the first `--` ends them. A value may not begin with a
//! dash (`--host=-x` says one that does). Where the JavaScript took options and
//! positionals in any order, the command here begins at the `--`, or at the
//! first word that is no option, and what follows is the command's own, whatever
//! it looks like.

use std::ffi::OsString;

use super::machine::{Machine, NAMES};
use super::script::{parse_env, Refusal};

/// The folder of the copy under the profile there, unless `--dir` names another.
pub const DEFAULT_DIR: &str = "consensflow-build";

/// What a run on the machine is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The machine, as `--machine` or `--host` named it: none for the default, chosen when the
    /// run goes ([`super::machine::choose`]).
    pub machine: Option<Machine>,
    /// Install the packages and build the page, a pane's cf.exe and the pane host first.
    pub build: bool,
    /// The folder of the copy, under the profile there.
    pub dir: String,
    /// The folder of the copy the command runs in.
    pub inside: String,
    /// The variables the command is given, and no others.
    pub env: Vec<(String, String)>,
    /// The command and its words, as they were given.
    pub command: Vec<String>,
}

/// What a matrix of evals on the machine is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixOptions {
    /// As [`Options`]'s.
    pub machine: Option<Machine>,
    pub build: bool,
    pub env: Vec<(String, String)>,
    pub scenarios: Vec<String>,
    pub pairs: Vec<Pair>,
    /// A Claude chief's model; the eval's own default when none.
    pub claude_model: Option<String>,
    /// A Pi member's model, on the machine's own signed-in provider.
    pub pi_model: Option<String>,
}

/// A chief and the staff it works with: `devin:devin`, `claude:devin,pi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub chief: String,
    pub staff: String,
}

impl Options {
    /// Reads the words after `windows`.
    pub fn parse(args: &[OsString]) -> Result<Self, String> {
        let words = read(args, &HOST_OPTIONS, Command::Allowed)?;
        let machine = machine_of(&words)?;
        if words.rest.is_empty() {
            return Err(USAGE.into());
        }
        let dir = words.one("dir").unwrap_or(DEFAULT_DIR);
        // Dots alone name no folder of its own: `..` would be the profile's parent.
        if !word_characters(dir, &['.']) || dir.chars().all(|c| c == '.') {
            return Err(format!("--dir is a folder name: {dir}"));
        }
        let inside = words.one("in").unwrap_or(".");
        if !word_characters(inside, &['.', '/']) || inside.split('/').any(|part| part == "..") {
            return Err(format!("--in is a folder of the copy: {inside}"));
        }
        Ok(Self {
            machine,
            build: words.flag("build"),
            dir: dir.to_owned(),
            inside: inside.to_owned(),
            env: named(&words)?,
            command: words.rest,
        })
    }
}

impl MatrixOptions {
    /// Reads the words after `windows-matrix`.
    pub fn parse(args: &[OsString]) -> Result<Self, String> {
        let words = read(args, &MATRIX_OPTIONS, Command::Refused)?;
        let machine = machine_of(&words)?;
        let scenarios = words.many("scenario");
        let pairs = words.many("pair");
        if scenarios.is_empty() || pairs.is_empty() {
            return Err(MATRIX_USAGE.into());
        }
        // Said well before the first run, not by the first one that fails on it.
        let env = named(&words)?;
        let pairs = pairs
            .iter()
            .map(|pair| {
                pair.split_once(':')
                    .filter(|(chief, staff)| !chief.is_empty() && !staff.is_empty())
                    .map(|(chief, staff)| Pair {
                        chief: chief.to_owned(),
                        staff: staff.to_owned(),
                    })
                    .ok_or_else(|| format!("a pair is chief:staff, not {pair}"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            machine,
            build: words.flag("build"),
            env,
            scenarios,
            pairs,
            claude_model: words.one("claude-model").map(str::to_owned),
            pi_model: words.one("pi-model").map(str::to_owned),
        })
    }
}

const USAGE: &str = "windows takes the command to run there after a --: \
    cargo xtask windows [--machine zeewin2|zeewin | --host <ssh host>] [--build] [--env NAME=VALUE] -- <command>";

const MATRIX_USAGE: &str = "windows-matrix takes a --scenario <name> and a \
    --pair <chief>:<staff> (each as many times as there are): cargo xtask windows-matrix \
    [--machine zeewin2|zeewin | --host <ssh host>] [--env NAME=VALUE] --scenario <name>... --pair <chief>:<staff>...";

/// The machine the words name: by `--machine`, one of [`NAMES`]; by `--host`, an ssh host
/// reached as zeewin is (it is given to `ssh` and `scp` as a word, so it may not be one that
/// they would take for an option), unless it is one of [`NAMES`], which is that machine with
/// its guards and its Node (the laptop is poker-lab's too); none when they name neither, and
/// never both.
fn machine_of(words: &Words) -> Result<Option<Machine>, String> {
    match (words.one("machine"), words.one("host")) {
        (Some(_), Some(_)) => Err("--machine and --host name the same thing: give one".into()),
        (Some(name), None) => Machine::named(name)
            .map(Some)
            .ok_or_else(|| format!("--machine is one of {}, not {name}", NAMES.join(", "))),
        (None, Some(host)) if host.starts_with('-') => Err(format!(
            "--host is an ssh host, not an option of ssh: {host}"
        )),
        (None, Some("")) => Err("--host is an ssh host, not an empty word".into()),
        (None, Some(host)) => Ok(Some(
            Machine::named(host).unwrap_or_else(|| Machine::at(host)),
        )),
        (None, None) => Ok(None),
    }
}

/// The variables the words name, as `parse_env` reads them.
fn named(words: &Words) -> Result<Vec<(String, String)>, String> {
    parse_env(&words.many("env")).map_err(|refusal: Refusal| refusal.to_string())
}

/// Whether `text` is one or more letters, digits and underscores (`\w` of a
/// regular expression, which is ASCII) and the characters `more`, and no other.
fn word_characters(text: &str, more: &[char]) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || more.contains(&c))
}

/// How many times an option may be given, and what it takes.
#[derive(Clone, Copy)]
enum Takes {
    /// A flag: it takes nothing.
    Nothing,
    /// A text; given twice, the last counts.
    One,
    /// A text, as many times as it is given.
    Many,
}

/// Whether the words end with a command.
#[derive(Clone, Copy)]
enum Command {
    Allowed,
    Refused,
}

const HOST_OPTIONS: [(&str, Takes); 6] = [
    ("machine", Takes::One),
    ("host", Takes::One),
    ("build", Takes::Nothing),
    ("dir", Takes::One),
    ("in", Takes::One),
    ("env", Takes::Many),
];

const MATRIX_OPTIONS: [(&str, Takes); 8] = [
    ("machine", Takes::One),
    ("host", Takes::One),
    ("build", Takes::Nothing),
    ("scenario", Takes::Many),
    ("pair", Takes::Many),
    ("claude-model", Takes::One),
    ("pi-model", Takes::One),
    ("env", Takes::Many),
];

/// What the words said: each option with the texts it was given (a flag, none), and what was left.
struct Words {
    given: Vec<(String, Vec<String>)>,
    rest: Vec<String>,
}

impl Words {
    fn flag(&self, name: &str) -> bool {
        self.given.iter().any(|(given, _)| given == name)
    }

    /// The text of an option that was given: the last, if it was given twice.
    fn one(&self, name: &str) -> Option<&str> {
        self.given
            .iter()
            .find(|(given, _)| given == name)
            .and_then(|(_, values)| values.last())
            .map(String::as_str)
    }

    /// Every text of an option, in the order given.
    fn many(&self, name: &str) -> Vec<String> {
        self.given
            .iter()
            .filter(|(given, _)| given == name)
            .flat_map(|(_, values)| values.iter().cloned())
            .collect()
    }
}

fn read(args: &[OsString], options: &[(&str, Takes)], command: Command) -> Result<Words, String> {
    let words: Vec<&str> = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .ok_or_else(|| format!("an argument is not UTF-8: {}", arg.to_string_lossy()))
        })
        .collect::<Result<_, _>>()?;
    let mut read = Words {
        given: Vec::new(),
        rest: Vec::new(),
    };
    let mut at = 0;
    while let Some(&word) = words.get(at) {
        at += 1;
        if word == "--" {
            read.rest = words[at..].iter().map(|word| (*word).to_owned()).collect();
            break;
        }
        let Some(spelled) = word.strip_prefix("--") else {
            if word.starts_with('-') && word.len() > 1 {
                return Err(format!("unknown option: {word}"));
            }
            return match command {
                Command::Allowed => {
                    read.rest = words[at - 1..]
                        .iter()
                        .map(|word| (*word).to_owned())
                        .collect();
                    Ok(read)
                }
                Command::Refused => Err(format!("unexpected argument: {word}")),
            };
        };
        let (name, inline) = match spelled.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (spelled, None),
        };
        let Some((name, takes)) = options.iter().find(|(known, _)| *known == name) else {
            return Err(format!("unknown option: --{name}"));
        };
        let values = match (takes, inline) {
            (Takes::Nothing, None) => Vec::new(),
            (Takes::Nothing, Some(_)) => return Err(format!("--{name} takes no value")),
            (_, Some(value)) => vec![value.to_owned()],
            (_, None) => {
                // A value may not look like an option: `--host --build` is a value missing.
                let value = words.get(at).filter(|next| !next.starts_with('-'));
                let Some(value) = value else {
                    return Err(format!(
                        "--{name} takes a value (to start one with a dash: --{name}=-value)"
                    ));
                };
                at += 1;
                vec![(*value).to_owned()]
            }
        };
        match read.given.iter_mut().find(|(given, _)| given == name) {
            Some((_, kept)) if matches!(takes, Takes::Many) => kept.extend(values),
            Some((_, kept)) => *kept = values,
            None => read.given.push(((*name).to_owned(), values)),
        }
    }
    Ok(read)
}

#[cfg(test)]
mod tests;
