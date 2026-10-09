//! `the cf commands ConsensFlow names`: every `cf …` that ConsensFlow's own
//! words name is a command cf has. A chief switched in was told to run `cf task
//! show T-1`, which cf refuses (2026-10-03). What cf has is read from its two
//! usages, the window's and the one outside a window, not from a list kept
//! here. `cf hook <harness>` is the one command neither names: the harnesses'
//! hooks run it, never a model.
//!
//! The JavaScript read the words with regular expressions, one of which looked
//! behind; the readers below are those expressions, and their own tests (at
//! the end) hold them to what the JavaScript's did, so that a reader that finds
//! nothing is not taken for a repository that is right.

use std::collections::{BTreeSet, HashMap};
use std::error::Error;

use cf_e2e::process::Run;
use cf_e2e::{cf, checkout, files, ScratchHome};
use regex::Regex;

use crate::Outcome;

/// Each command and its verbs (none: it takes arguments, not a verb).
type Commands = HashMap<String, Option<BTreeSet<String>>>;

/// Each command and its verbs, from the lines of a usage; `prefix` is what
/// stands before the command in its lines.
fn commands_of(usage: &str, prefix: &str) -> Result<Commands, regex::Error> {
    let line = Regex::new(&format!(r"^\s+{prefix}([a-z]+)(?: \[?([a-z|]+)(?-u:\b))?"))?;
    let mut commands = Commands::new();
    for text in usage.split('\n') {
        let Some(found) = line.captures(text) else {
            continue;
        };
        let command = commands.entry(found[1].to_owned()).or_insert(None);
        if let Some(verbs) = found.get(2) {
            command
                .get_or_insert_with(BTreeSet::new)
                .extend(verbs.as_str().split('|').map(str::to_owned));
        }
    }
    Ok(commands)
}

/// The usage `cf help` prints outside a window. It is run here, not in a
/// window: a window's token makes cf its board.
fn outside_usage() -> Result<String, Box<dyn Error>> {
    let home = ScratchHome::new()?;
    let ran = Run::new(cf::binary()?)
        .arg("help")
        .cwd(checkout::root())
        .inheriting_env()
        .vars(home.vars())
        .var("CONSENSFLOW_TOKEN", "")
        .run()?;
    assert_eq!(ran.code, Some(0), "{ran}");
    Ok(ran.stdout)
}

/// What cf has: the commands of the usage outside a window and of the window's
/// (the text the native cf prints inside one), the window's winning where both
/// have one, and the two that are named in neither.
fn commands() -> Result<Commands, Box<dyn Error>> {
    let window = files::read_string(&checkout::path("crates/cf/src/board/usage.txt"))?;
    let mut commands = commands_of(&outside_usage()?, "")?;
    commands.extend(commands_of(&window, "cf ")?);
    commands.insert("help".to_owned(), None);
    commands.insert("hook".to_owned(), None);
    Ok(commands)
}

/// Which files have words that reach a model or a person: the native crates'
/// sources and texts (the daemon, the engine's role texts, the cf usages, the
/// agents pages), the host extensions, role texts, the eval prompts, the page,
/// the readme. A crate's own tests are left out, since they say what cf
/// refuses too.
struct Readers {
    in_a_crate: Regex,
    a_test: Regex,
    outside_the_crates: Regex,
}

impl Readers {
    fn new() -> Result<Self, regex::Error> {
        Ok(Self {
            in_a_crate: Regex::new(r"^crates/[^/]+/src/.*\.(rs|txt|md|html)$")?,
            a_test: Regex::new(r"(^|/)(tests|testing)(/|\.rs$)")?,
            outside_the_crates: Regex::new(r"\.(js|mjs|md|html)$")?,
        })
    }

    /// Whether the words of `file`, written from the checkout's root with `/`
    /// between folders, reach a reader.
    fn reach(&self, file: &str) -> bool {
        let reaches = if file.starts_with("crates/") {
            self.in_a_crate.is_match(file)
                && !self
                    .a_test
                    .is_match(&file[file.find("/src/").unwrap_or(0)..])
        } else {
            self.outside_the_crates.is_match(file)
        };
        reaches
            && !file.contains("node_modules/")
            && !file.starts_with("app/ui/vendor/")
            && !file.starts_with("evals/reports/")
    }
}

/// The files whose words reach a reader, read from the folders (the gate's tree
/// has no .git).
fn files_that_reach_a_reader() -> Result<Vec<String>, Box<dyn Error>> {
    let readers = Readers::new()?;
    let mut found = vec!["README.md".to_owned()];
    for root in ["crates", "hosts", "skill", "app/ui", "evals"] {
        for file in checkout::files_below(&checkout::path(root), &["node_modules"])? {
            found.push(checkout::relative(&file));
        }
    }
    found.retain(|file| readers.reach(file));
    Ok(found)
}

/// Whether `before`, the character ahead of a `cf`, makes it the end of
/// something longer: a word, a path, a flag.
fn ends_a_longer_word(before: Option<char>) -> bool {
    before.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '$' | '-'))
}

/// Where the words of a file name a command or a verb that cf does not have.
struct Mentions {
    mention: Regex,
}

impl Mentions {
    fn new() -> Result<Self, regex::Error> {
        Ok(Self {
            mention: Regex::new(r"(`?)cf ([a-z]+)(?: ([a-z]+(?:\|[a-z]+)*))?")?,
        })
    }

    /// The mentions in `text`, the words of `file`, of a command or a verb that
    /// `commands` do not have, as `file:line: cf command verb`.
    fn wrong_in(&self, file: &str, text: &str, commands: &Commands) -> Vec<String> {
        let mut wrong = Vec::new();
        let mut from = 0;
        while let Some(found) = self.mention.captures_at(text, from) {
            let (Some(whole), Some(quoted), Some(command)) =
                (found.get(0), found.get(1), found.get(2))
            else {
                break;
            };
            // A `cf` that is the end of a longer word is none; the search goes on
            // just past where it began, as a regular expression's does.
            if quoted.as_str().is_empty()
                && ends_a_longer_word(text[..whole.start()].chars().next_back())
            {
                from = whole.start() + 1;
                continue;
            }
            from = whole.end();
            let line = text[..whole.start()].matches('\n').count() + 1;
            let at = format!("{file}:{line}");
            let Some(known) = commands.get(command.as_str()) else {
                // Prose may say "cf is the board"; in Markdown a quoted command
                // must be one (in code a backtick opens a template string).
                if !quoted.as_str().is_empty() && file.ends_with(".md") {
                    wrong.push(format!("{at}: cf {}", command.as_str()));
                }
                continue;
            };
            let (Some(known), Some(verbs)) = (known, found.get(3)) else {
                continue;
            };
            for verb in verbs.as_str().split('|') {
                if !known.contains(verb) {
                    wrong.push(format!("{at}: cf {} {verb}", command.as_str()));
                }
            }
        }
        wrong
    }
}

#[test]
fn knows_the_window_commands_and_the_ones_outside_a_window() -> Outcome {
    let commands = commands()?;
    let task = commands
        .get("task")
        .and_then(Option::as_ref)
        .ok_or("task is a command with verbs")?;
    assert_eq!(
        task.iter().map(String::as_str).collect::<Vec<_>>(),
        ["accept", "add", "cancel", "done", "get", "list", "pause", "reopen", "resume"]
    );
    for command in [
        "inbox", "ask", "answer", "tell", "history", "agent", "setup", "doctor",
    ] {
        assert!(commands.contains_key(command), "{command}");
    }
    Ok(())
}

#[test]
fn names_only_commands_and_verbs_cf_has() -> Outcome {
    let (commands, mentions) = (commands()?, Mentions::new()?);
    let mut wrong = Vec::new();
    for file in files_that_reach_a_reader()? {
        let bytes = files::read(&checkout::path(&file))?;
        wrong.extend(mentions.wrong_in(&file, &String::from_utf8_lossy(&bytes), &commands));
    }
    assert_eq!(wrong, Vec::<String>::new());
    Ok(())
}

// Not cases of the JavaScript: the readers above, held to what its regular
// expressions did.

/// A few commands, as the usages give them: `task` with verbs, `agent` and
/// `catalog` without, and `help`.
fn known() -> Commands {
    let mut commands = Commands::new();
    commands.insert(
        "task".to_owned(),
        Some(["add", "list"].map(str::to_owned).into()),
    );
    for command in ["agent", "catalog", "help"] {
        commands.insert(command.to_owned(), None);
    }
    commands
}

fn wrong_in(file: &str, text: &str) -> Result<Vec<String>, regex::Error> {
    Ok(Mentions::new()?.wrong_in(file, text, &known()))
}

#[test]
fn a_verb_cf_does_not_have_is_named_with_its_file_and_line() -> Outcome {
    let text = "first\nrun `cf task show T-1` now\ncf task list and cf task add|nope\n";
    assert_eq!(
        wrong_in("a.md", text)?,
        ["a.md:2: cf task show", "a.md:3: cf task nope"]
    );
    // Commands that take no verb, and verbs cf has, are right.
    assert_eq!(
        wrong_in("a.rs", "cf agent anything, cf task add|list, cf help me")?,
        Vec::<String>::new()
    );
    Ok(())
}

#[test]
fn a_quoted_command_cf_does_not_have_is_wrong_in_markdown_alone() -> Outcome {
    let text = "`cf frobnicate` and cf is the board and cf frobnicate\n";
    assert_eq!(wrong_in("a.md", text)?, ["a.md:1: cf frobnicate"]);
    assert_eq!(wrong_in("a.rs", text)?, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_cf_that_ends_a_longer_word_a_path_or_a_flag_is_none() -> Outcome {
    for text in [
        "xcf task show",
        "_cf task show",
        "./cf task show",
        "bin/cf task show",
        "$cf task show",
        "--cf task show",
        "9cf task show",
    ] {
        assert_eq!(wrong_in("a.md", text)?, Vec::<String>::new(), "{text}");
    }
    // Ahead of one that begins a word, and ahead of one in backticks, it is a mention.
    for text in [
        "a cf task show",
        "(cf task show",
        "`cf task show",
        "\"cf task show",
    ] {
        assert_eq!(wrong_in("a.md", text)?, ["a.md:1: cf task show"], "{text}");
    }
    Ok(())
}

#[test]
fn the_search_goes_on_past_a_cf_that_was_none() -> Outcome {
    // The first match, `cf cf`, is the end of a word; the second begins inside it.
    assert_eq!(
        wrong_in("a.md", "xcf cf task show")?,
        ["a.md:1: cf task show"]
    );
    Ok(())
}

#[test]
fn the_usages_give_commands_with_the_verbs_of_their_lines() -> Outcome {
    let outside =
        "Usage:\n  catalog [--json]\n  agent list|add [name]\n  agent edit <name>\n  setup\n";
    let commands = commands_of(outside, "")?;
    assert_eq!(commands["catalog"], None);
    assert_eq!(
        verbs_of(&commands, "agent"),
        Some(vec!["add", "edit", "list"])
    );
    assert_eq!(commands["setup"], None);
    // A window's lines say `cf` before the command.
    let window = "  cf task add|list [options]\n  cf inbox\n  not a command line\n";
    let commands = commands_of(window, "cf ")?;
    assert_eq!(verbs_of(&commands, "task"), Some(vec!["add", "list"]));
    assert_eq!(commands["inbox"], None);
    assert!(!commands.contains_key("not"));
    Ok(())
}

/// The verbs of `command`, in order, if it has any.
fn verbs_of<'a>(commands: &'a Commands, command: &str) -> Option<Vec<&'a str>> {
    commands[command]
        .as_ref()
        .map(|verbs| verbs.iter().map(String::as_str).collect())
}

#[test]
fn the_files_that_reach_a_reader_are_the_texts_of_the_product_and_not_its_tests() -> Outcome {
    let readers = Readers::new()?;
    for file in [
        "README.md",
        "crates/cf/src/lib.rs",
        "crates/cf/src/board/usage.txt",
        "crates/cf-daemon/src/screens/page.html",
        "hosts/pi/consensflow.js",
        "skill/chief.md",
        "app/ui/core/page.js",
        "evals/run.mjs",
    ] {
        assert!(readers.reach(file), "{file}");
    }
    for file in [
        "crates/cf/tests/standalone.rs",
        "crates/cf/src/standalone/tests.rs",
        "crates/cf-harness/src/testing/mod.rs",
        "crates/cf/src/tests/helpers.rs",
        "crates/cf/Cargo.toml",
        "crates/cf/README.md",
        "crates/cf/src/lib.toml",
        "app/ui/vendor/xterm.js",
        "app/ui/node_modules/x/index.js",
        "evals/reports/run.md",
        "skill/chief.txt",
        "package.json",
    ] {
        assert!(!readers.reach(file), "{file}");
    }
    Ok(())
}
