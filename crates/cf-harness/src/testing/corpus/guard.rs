//! The guard of the corpus's data (T-1243, the third review of `afeb0f595`): nothing of the owner's, of
//! their accounts or of their machine is in a file the branch adds or changes under test.
//!
//! The recorded screens are the harnesses' real ones, drawn on the owner's machine in the owner's accounts.
//! They were scrubbed with values of the same width that are none of the owner's, and the first guard
//! checked six substrings in three data files, which a review defeated four times over (the owner's name,
//! the original credit row, the original reset row, an email address). This one is a function of a line,
//! per class of what the scrub removed, and a test per class puts a contaminated variant in and sees it
//! found:
//!
//! - **name**: the owner's name, in any case and inside any word or path. Held as hashes of its lowercase
//!   forms (`NAMES`), so that no file of the repository spells it, this one included;
//! - **path**: a path through a home or a temporary folder of a machine, but for the synthetic ones the
//!   scrub writes (`FOLDERS`), and the names of the worktrees' folders (`WORKTREES`);
//! - **id**: a session id (a uuid but for the synthetic ones, `SYNTHETIC_ID`), the stamp of a run (but for
//!   `SYNTHETIC_RUN`) and any run of 20 hex digits that is not one digit repeated;
//! - **email**: an address of any domain;
//! - **key**: the prefixes of the keys and tokens the services issue, followed by their characters;
//! - **figure**: a usage, context, credit, quota or reset figure. A row that holds one is held to the
//!   scrub's synthetic value: every digit of the figure is a 5, and a reset is one of two phrases (`RESETS`).
//!   The shapes are the real rows' (Codex's `/status`, Devin's footer and usage row, Pi's stats line,
//!   Claude Code's spinner), so a real row put back is found whatever its figures are. And any number
//!   with a unit (`k`, `M`, `%`) is held to the same digits wherever it stands, with its label on the line
//!   or on another: a review found the original context figures in constants of this file, with their
//!   labels in the rows that used them, and no rule of labels could see them. A bare number on a line of
//!   its own has no unit and no label, so no rule of the shape can see it: the originals are searched for
//!   by a script that is run, and the figures a control uses are invented ones that match no original;
//! - **plan**: the plan an account is on, in Claude Code's header and in Devin's usage row: the synthetic
//!   `Xxx`, and no real plan name after `Claude `;
//! - **account**: the account rows, by their labels (`ACCOUNT_LABELS`): the account's is `<redacted>`, and
//!   an email, an organization or a login row is none.
//!
//! [`scan`] says which lines of a text it finds and why. The tests: the files below hold none, and each
//! class finds its contaminated variants (built here from clean rows with a digit changed, from pieces, or
//! from bytes, so that no contaminated value is in the text of this file, which is scanned too).

use std::fs;
use std::path::{Path, PathBuf};

/// What a finding is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    Name,
    Path,
    Id,
    Email,
    Key,
    Figure,
    Plan,
    Account,
}

/// One line of a text that a class found, and why.
#[derive(Debug)]
pub(super) struct Finding {
    pub(super) class: Class,
    pub(super) line: usize,
    pub(super) why: String,
}

/// A class's check of one line: what it finds in it.
type Check = fn(&str) -> Vec<String>;

/// Every class and its check. [`scan`] applies those of the classes it is asked for.
const CHECKS: &[(Class, Check)] = &[
    (Class::Name, names),
    (Class::Path, paths),
    (Class::Id, ids),
    (Class::Email, emails),
    (Class::Key, keys),
    (Class::Figure, figures),
    (Class::Plan, plans),
    (Class::Account, accounts),
];

/// Every class.
const ALL: &[Class] = &[
    Class::Name,
    Class::Path,
    Class::Id,
    Class::Email,
    Class::Key,
    Class::Figure,
    Class::Plan,
    Class::Account,
];

/// What `text` holds of the `classes`, line by line.
pub(super) fn scan(text: &str, classes: &[Class]) -> Vec<Finding> {
    let mut found = Vec::new();
    for (at, line) in text.lines().enumerate() {
        for (class, check) in CHECKS {
            if !classes.contains(class) {
                continue;
            }
            for why in check(line) {
                found.push(Finding {
                    class: *class,
                    line: at + 1,
                    why,
                });
            }
        }
    }
    found
}

// ─── name ──────────────────────────────────────────────────────────────────────────────────────

/// The owner's names as (length, FNV-1a hash of the lowercase letters): a window of a line that is
/// one of them is the name, whatever is round it.
const NAMES: [(usize, u64); 2] = [(5, 0x8296_2682_89c5_0429), (7, 0x5597_ff95_a6f5_89fd)];

/// FNV-1a, 64 bits.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn names(line: &str) -> Vec<String> {
    let lower: Vec<u8> = line.bytes().map(|byte| byte.to_ascii_lowercase()).collect();
    NAMES
        .iter()
        .filter(|(len, hash)| lower.windows(*len).any(|window| fnv(window) == *hash))
        .map(|(len, _)| format!("a word of {len} letters that is the owner's name"))
        .collect()
}

// ─── path ──────────────────────────────────────────────────────────────────────────────────────

/// The folders a path of a machine goes through, each with what may follow it in a synthetic path.
const FOLDERS: &[(&str, &str)] = &[
    ("Users", ""),
    ("home", "user/"),
    ("var/folders", "xx/"),
    ("private/var", ""),
    ("private/tmp", ""),
    ("tmp", "consensflow/"),
];

/// The names of the worktrees' folders, in two parts so that this file does not spell them.
const WORKTREES: &[(&str, &str)] = &[
    ("consensflow", "-trees"),
    (".runs", "/"),
    ("release", "-check-"),
];

fn paths(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (folder, allowed) in FOLDERS {
        let needle = format!("/{folder}/");
        for (at, _) in line.match_indices(&needle) {
            let rest = &line[at + needle.len()..];
            if allowed.is_empty() || !rest.starts_with(allowed) {
                found.push(format!("a path through {needle}"));
            }
        }
    }
    let windows = format!("\\{}\\", "Users");
    if line.contains(&windows) {
        found.push(format!("a path through {windows}"));
    }
    for (head, tail) in WORKTREES {
        let name = format!("{head}{tail}");
        if line.contains(&name) {
            found.push(format!("the name {name} of a worktree's folder"));
        }
    }
    found
}

// ─── id ────────────────────────────────────────────────────────────────────────────────────────

/// What a session id begins with in the scrub's synthetic ones.
const SYNTHETIC_ID: &str = "00000000-0000-4000-8000-";

/// The scrub's synthetic stamp of a run.
const SYNTHETIC_RUN: &str = "20260101T000000Z";

fn ids(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    // A uuid: 8-4-4-4-12 hex digits.
    if bytes.len() >= 36 {
        for at in 0..=bytes.len() - 36 {
            let window = &bytes[at..at + 36];
            let shaped = window.iter().enumerate().all(|(place, byte)| {
                if [8, 13, 18, 23].contains(&place) {
                    *byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            });
            if shaped && !line[at..].starts_with(SYNTHETIC_ID) {
                found.push("a session id (a uuid)".to_owned());
            }
        }
    }
    // A run's stamp: eight digits, T, six digits, Z.
    if bytes.len() >= 16 {
        for at in 0..=bytes.len() - 16 {
            let window = &bytes[at..at + 16];
            let shaped = window[8] == b'T'
                && window[15] == b'Z'
                && window[..8].iter().all(u8::is_ascii_digit)
                && window[9..15].iter().all(u8::is_ascii_digit);
            if shaped && &line[at..at + 16] != SYNTHETIC_RUN {
                found.push("the stamp of a run".to_owned());
            }
        }
    }
    // A long run of hex digits that is not one digit repeated, and no number written `0x…`.
    let mut at = 0;
    while at < bytes.len() {
        if !bytes[at].is_ascii_hexdigit() {
            at += 1;
            continue;
        }
        let end = at
            + bytes[at..]
                .iter()
                .take_while(|byte| byte.is_ascii_hexdigit())
                .count();
        let run = &bytes[at..end];
        let hex_number = at >= 2 && &bytes[at - 2..at] == b"0x";
        if run.len() >= 20 && run.iter().any(|byte| *byte != run[0]) && !hex_number {
            found.push(format!("a run of {} hex digits", run.len()));
        }
        at = end;
    }
    found
}

// ─── email ─────────────────────────────────────────────────────────────────────────────────────

fn emails(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in line.match_indices('@') {
        let local = line[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || "._%+-".contains(*c))
            .count();
        if local == 0 {
            continue;
        }
        let domain: String = line[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        let labels: Vec<&str> = domain.split('.').collect();
        let last_is_a_tld = labels.last().is_some_and(|label| {
            label.len() >= 2 && label.chars().all(|c| c.is_ascii_alphabetic())
        });
        if labels.len() >= 2 && labels.iter().all(|label| !label.is_empty()) && last_is_a_tld {
            found.push("an email address".to_owned());
        }
    }
    found
}

// ─── key ───────────────────────────────────────────────────────────────────────────────────────

/// The prefixes of the keys and tokens the services issue, and how many characters of a key follow.
const KEYS: &[(&str, usize)] = &[
    ("sk-", 10),
    ("ghp_", 10),
    ("xoxb-", 10),
    ("xoxp-", 10),
    ("eyJ", 10),
    ("AKIA", 16),
    ("Bearer ", 20),
];

fn keys(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (prefix, most) in KEYS {
        for (at, _) in line.match_indices(prefix) {
            let after_a_word = line[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
            let run = line[at + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c))
                .count();
            if !after_a_word && run >= *most {
                found.push(format!("a key that begins {prefix}"));
            }
        }
    }
    found
}

// ─── figure ────────────────────────────────────────────────────────────────────────────────────

/// What a reset reads in the scrub's synthetic rows: the time and the day, or the hours and minutes.
const RESETS: [&str; 2] = ["12:34 on 12 Jan", "in 12h 34m"];

/// Whether every digit of `figure` is a 5, the scrub's synthetic digit.
fn fives(figure: &str) -> bool {
    figure
        .chars()
        .filter(char::is_ascii_digit)
        .all(|c| c == '5')
}

/// The digits and letters of a number written right before `at` in `line` (`555k`, `55`).
fn number_before(line: &str, at: usize) -> String {
    let mut number: Vec<char> = line[..at]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || ".kKmM".contains(*c))
        .collect();
    number.reverse();
    number.into_iter().collect()
}

/// The number written right after `at` in `line`.
fn number_after(line: &str, at: usize) -> String {
    line[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect()
}

/// Whether every digit of `figure` is a 5 or a 0.
fn fives_or_zeros(figure: &str) -> bool {
    figure
        .chars()
        .filter(char::is_ascii_digit)
        .all(|c| c == '5' || c == '0')
}

/// The numbers of `line` written with a unit right after them (`k`, `K`, `M` or `%`, and nothing that
/// goes on a word), each with its unit.
fn unit_figures(line: &str) -> Vec<(&str, char)> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let starts = bytes[at].is_ascii_digit()
            && (at == 0 || !(bytes[at - 1].is_ascii_digit() || bytes[at - 1] == b'.'));
        if !starts {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end + 1 < bytes.len() && bytes[end] == b'.' && bytes[end + 1].is_ascii_digit() {
            end += 1;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
        }
        let unit = bytes.get(end).copied();
        let ends = bytes
            .get(end + 1)
            .is_none_or(|next| !(next.is_ascii_alphanumeric() || *next == b'_'));
        if let Some(unit @ (b'k' | b'K' | b'M' | b'%')) = unit {
            if ends {
                found.push((&line[at..end], char::from(unit)));
            }
        }
        at = end.max(at + 1);
    }
    found
}

/// `figure` without the synthetic resets.
fn without_resets(figure: &str) -> String {
    RESETS
        .iter()
        .fold(figure.to_owned(), |text, reset| text.replace(reset, ""))
}

fn figures(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut held = |what: &str, figure: &str| {
        if !fives(&without_resets(figure)) {
            found.push(format!("{what}: {}", figure.trim()));
        }
    };
    // The rows that hold figures, from their label to the end of the row: Devin's footer, Codex's
    // `/status`.
    for label in ["Context:", "Context window:", "Weekly limit:", "Credits:"] {
        if let Some(at) = line.find(label) {
            held(label, &line[at..]);
        }
    }
    // The figure written next to its unit, wherever the row is.
    for unit in ["% left", "% remaining", " credits", " tokens"] {
        for (at, _) in line.match_indices(unit) {
            held(unit.trim(), &number_before(line, at));
        }
    }
    // What a quota says it has used, in the parentheses of Codex's row of the context window.
    if let Some(at) = line.find(" used /") {
        let from = line[..at].rfind('(').unwrap_or(0);
        held("used", &line[from..]);
    }
    // A sum of money.
    for (at, _) in line.match_indices('$') {
        held("a sum", &number_after(line, at + 1));
    }
    // Pi's line of stats: the tokens, the cache, the cost and the context, before `(auto)`.
    if let Some(at) = line.find("(auto)").filter(|_| line.contains('↑')) {
        held("Pi's stats", &line[..at]);
    }
    // A number with a unit, wherever it stands: followed by `k`, `K`, `M` or `%`, with its label on the
    // line, on another or none. The scrub's are all 5s; zero says nothing. A letter may be glued before
    // it (Pi writes its cache's figures so), a digit or a point may not.
    for (number, unit) in unit_figures(line) {
        if !fives_or_zeros(number) {
            found.push(format!("a figure with a unit: {number}{unit}"));
        }
    }
    // A reset is one of the two synthetic phrases.
    for (at, _) in line.match_indices("resets") {
        let phrase = line[at + "resets".len()..]
            .split(')')
            .next()
            .unwrap_or("")
            .trim();
        if phrase.chars().any(|c| c.is_ascii_digit()) && !RESETS.contains(&phrase) {
            found.push(format!("a reset: {phrase}"));
        }
    }
    found
}

// ─── plan ──────────────────────────────────────────────────────────────────────────────────────

/// The plans an account can be on, which no synthetic row names.
const PLANS: &[&str] = &["Free", "Pro", "Max", "Team", "Teams", "Enterprise", "Plus"];

/// What the scrub puts where the plan was: a word that is no plan.
const NO_PLAN: &str = "Xxx";

fn plans(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let lower = line.to_lowercase();
    for plan in PLANS {
        let phrase = format!("claude {}", plan.to_lowercase());
        for (at, _) in lower.match_indices(&phrase) {
            let ends = lower[at + phrase.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            if ends {
                found.push(format!("the plan {plan} named after Claude"));
            }
        }
    }
    // Claude Code's header: `… with low effort · Claude <plan>`.
    let header = "effort · Claude ";
    if let Some(at) = line.find(header) {
        let word: String = line[at + header.len()..]
            .chars()
            .take_while(|c| c.is_alphanumeric())
            .collect();
        if !word.is_empty() && word != NO_PLAN {
            found.push(format!("a plan in Claude Code's header: {word}"));
        }
    }
    // Devin's usage row: the plan, a dot, the share that remains and when it resets.
    if let Some(at) = line.find("% remaining") {
        if let Some(dot) = line[..at].find(" · ") {
            let plan = line[..dot].split_whitespace().next_back().unwrap_or("");
            if plan != NO_PLAN {
                found.push(format!("a plan in Devin's usage row: {plan}"));
            }
        }
    }
    found
}

// ─── account ───────────────────────────────────────────────────────────────────────────────────

/// The rows that name an account, by their labels; the account's own is `<redacted>` in the recorded
/// screens, and the others are none.
const ACCOUNT_LABELS: &[&str] = &["Account", "Email", "E-mail", "Organization", "Login method"];

fn accounts(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for label in ACCOUNT_LABELS {
        let marker = format!("{label}:");
        if let Some(at) = line.find(&marker) {
            let rest = line[at + marker.len()..].trim();
            // The account's row says `<redacted>` and nothing else: what follows it may be the
            // quotes and the comma of a line of source, never a word.
            let redacted = *label == "Account"
                && rest
                    .strip_prefix("<redacted>")
                    .is_some_and(|more| !more.chars().any(char::is_alphanumeric));
            if rest.chars().any(char::is_alphanumeric) && !redacted {
                found.push(format!("a row that names an account: {marker}"));
            }
        }
    }
    found
}

// ─── the files ─────────────────────────────────────────────────────────────────────────────────

/// The folder of the crate (`crates/cf-harness`).
fn crate_folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file under `folder`, whatever it is, in order.
fn files_under(folder: &Path, into: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(folder)
        .unwrap_or_else(|failed| panic!("{}: {failed}", folder.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|failed| panic!("{}: {failed}", folder.display()))
                .path()
        })
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            files_under(&entry, into);
        } else {
            into.push(entry);
        }
    }
}

/// Every class but the ids: for the files of the base that the branch changed, which hold uuids that
/// their tests made up long before (the guard cannot tell them from a session's).
const NO_IDS: &[Class] = &[
    Class::Name,
    Class::Path,
    Class::Email,
    Class::Key,
    Class::Figure,
    Class::Plan,
    Class::Account,
];

/// The files the branch adds or changes under test, each with the classes it is held to: everything
/// under `src/testing` (the corpus, the fixtures, the frozen reader and this guard), the other files the
/// branch adds (the tests of Codex's, Devin's and Pi's windows, the stand-in Codex's screen, the rig case),
/// the files of the base it changed (all but the ids), and the plants' table, whose `replace` texts hold
/// contaminated values on purpose and so is held to the name and the keys alone. A contaminated value
/// there is an invented one, never a real value put back (a plant that did so was found by a scan of the
/// history, not by this guard: the limit of holding that file to two classes).
fn branch_files() -> Vec<(PathBuf, &'static [Class])> {
    let crate_folder = crate_folder();
    let mut under_testing = Vec::new();
    files_under(
        &crate_folder.join("src").join("testing"),
        &mut under_testing,
    );
    let mut files: Vec<(PathBuf, &'static [Class])> =
        under_testing.into_iter().map(|file| (file, ALL)).collect();
    let beside: [(&[&str], &'static [Class]); 11] = [
        (&["tests", "launch", "codex", "dialogs.rs"], ALL),
        (&["tests", "launch", "devin", "dialogs.rs"], ALL),
        (&["tests", "launch", "pi", "dialogs.rs"], ALL),
        (
            &["..", "cf-e2e", "src", "bin", "fake_codex", "screen.rs"],
            ALL,
        ),
        (
            &[
                "..",
                "cf-e2e",
                "tests",
                "regressions",
                "a_codex_chiefs_idle_prompt_was_read_as_a_dialog.rs",
            ],
            ALL,
        ),
        (&["src", "shared", "dialog", "tests.rs"], NO_IDS),
        (&["src", "muse", "window", "tests.rs"], NO_IDS),
        (&["tests", "launch", "claude", "dialogs.rs"], NO_IDS),
        (
            &[
                "..",
                "cf-engine",
                "tests",
                "dispatcher",
                "harness_dialogs.rs",
            ],
            NO_IDS,
        ),
        (&["..", "cf-proto", "src", "panes.rs"], NO_IDS),
        (
            &[
                "..",
                "..",
                "tests",
                "plants",
                "daemon",
                "harness-dialogs.toml",
            ],
            &[Class::Name, Class::Key],
        ),
    ];
    for (parts, classes) in beside {
        let file = parts
            .iter()
            .fold(crate_folder.clone(), |path, part| path.join(part));
        files.push((file, classes));
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows the scrub leaves, one of each shape the figure class holds to a synthetic value.
    const CLEAN_FIGURES: &[&str] = &[
        "SWE-2 Medium                       Context: 55k / 555k tokens (5%)",
        "  Context window:      55% left (55.5K used / 555K)",
        "  Weekly limit:        [██████████░░░░░░░░░░] 55% left (resets 12:34 on 12 Jan)",
        "  Credits:             55555 credits",
        "  Xxx · 555% remaining (resets in 12h 34m)",
        "↑5.5k ↓555 R5.5k CH55.5% $5.555 5.5%/5.5M (auto)        meta/muse-spark-1.3 • low",
        "✽ Honking… (2s · ↓ 55 tokens)",
    ];

    /// Figures that are not the scrub's, kept on lines of their own (a row that held one with its label
    /// would be found in this file).
    const OTHER_CREDITS: &str = "48213";
    const OTHER_RESET: &str = "09:41 on 3 Nov";
    const OTHER_CONTEXT: [&str; 2] = ["19", "287"];
    const OTHER_LEFT: &str = "71";
    const OTHER_USED: &str = "41.7";
    const OTHER_TOTAL: &str = "312";
    const OTHER_SUM: &str = "0.42";
    const OTHER_COUNT: &str = "2";

    /// A context as Devin's footer writes it, of invented figures: the unit is written between the
    /// pieces, so that no line of this file holds a figure with a unit.
    fn other_context() -> String {
        format!("{}k / {}k", OTHER_CONTEXT[0], OTHER_CONTEXT[1])
    }

    /// `row` with its first digit that is a 5 turned into a 7: a figure that is not the scrub's.
    fn contaminated(row: &str) -> String {
        row.replacen('5', "7", 1)
    }

    fn classes_found(text: &str) -> Vec<Class> {
        let mut classes: Vec<Class> = scan(text, ALL).iter().map(|found| found.class).collect();
        classes.dedup();
        classes
    }

    fn found_in(class: Class, text: &str) -> bool {
        scan(text, ALL).iter().any(|found| found.class == class)
    }

    /// The owner's name, from bytes: no file of the repository spells it.
    fn the_owners_names() -> [String; 2] {
        [
            String::from_utf8(vec![118, 111, 105, 99, 117]).unwrap(),
            String::from_utf8(vec![103, 97, 98, 114, 105, 101, 108]).unwrap(),
        ]
    }

    /// The files the branch adds or changes under test hold nothing of the owner's, of their accounts or
    /// of their machine: the recorded screens, Pi's renderings, the fixtures, the corpus, the frozen reader
    /// and this guard, and the tests round them.
    #[test]
    fn the_files_the_branch_adds_under_test_hold_nothing_of_the_owners() {
        let files = branch_files();
        let names: Vec<String> = files
            .iter()
            .map(|(file, _)| file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        for expected in [
            "recorded.txt",
            "pi_rendered.txt",
            "bodies.txt",
            "screens.rs",
            "corpus.rs",
            "base_dialog.rs",
            "guard.rs",
            "mod.rs",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "{expected} is not scanned"
            );
        }
        let mut found = Vec::new();
        for (file, classes) in &files {
            let text = fs::read_to_string(file)
                .unwrap_or_else(|failed| panic!("{}: {failed}", file.display()));
            for finding in scan(&text, classes) {
                found.push(format!(
                    "{}:{}: {:?}: {}",
                    file.display(),
                    finding.line,
                    finding.class,
                    finding.why
                ));
            }
        }
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    /// The scrub's own rows are clean: the guard is not a rule that holds nothing.
    #[test]
    fn the_scrubs_synthetic_rows_are_clean() {
        for row in CLEAN_FIGURES {
            assert_eq!(classes_found(row), Vec::<Class>::new(), "{row}");
        }
        for row in [
            "  Session:             00000000-0000-4000-8000-000000000001",
            "  /home/user/work/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/ws",
            "  /var/folders/xx/yyyyyyyyyyyyyyyyyyyyyyyyyyyyyy/T/consensflow-integration-abcdef",
            "  Account:             <redacted>",
            "▝▜██████▀  Sonnet 5.5 with low effort · Claude Xxx",
            "SWE-2 Max                                   See usage and cost: /session-stats",
            "5. More reasoning…   Max consumes usage limits faster",
            "  m-3 answered @worker.",
            "const HASH: u64 = 0x60cfc152347ba0cb;",
        ] {
            assert_eq!(classes_found(row), Vec::<Class>::new(), "{row}");
        }
    }

    #[test]
    fn the_name_is_found_in_any_case_inside_any_word_or_path() {
        for name in the_owners_names() {
            assert_eq!(fnv(name.as_bytes()), {
                let table = NAMES.iter().find(|(len, _)| *len == name.len()).unwrap();
                table.1
            });
            let capital = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
            for text in [
                name.clone(),
                capital,
                name.to_uppercase(),
                format!("xx{name}yy"),
                format!("/work/{name}/project"),
                format!("by {name} on 2026-10-29"),
            ] {
                assert!(found_in(Class::Name, &text), "{text}");
            }
        }
    }

    #[test]
    fn a_path_of_a_machine_or_a_worktree_is_found() {
        let [name, _] = the_owners_names();
        for text in [
            format!("/{}/{name}/work", "Users"),
            format!("/{}/someone/work", "home"),
            format!("/{}/ab/cdefgh/T/x", "var/folders"),
            format!("/{}/xyz", "private/var"),
            format!("/{}/someone-else/x", "tmp"),
            format!("C:\\{}\\someone\\x", "Users"),
            format!("see {}{} for it", "consensflow", "-trees"),
            format!("({}/{})", ".runs", "T-x"),
            format!("{}{}mac-a2", "release", "-check-"),
        ] {
            assert!(found_in(Class::Path, &text), "{text}");
        }
        for clean in [
            format!("/{}/user/work/aaaa", "home"),
            format!("/{}/xx/yyyy/T/x", "var/folders"),
            format!("/{}/consensflow/bundle/bin/cf", "tmp"),
        ] {
            assert!(!found_in(Class::Path, &clean), "{clean}");
        }
    }

    #[test]
    fn a_session_id_a_run_stamp_or_a_long_hex_run_is_found() {
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            "01234567", "89ab", "cdef", "0123", "456789abcdef"
        );
        let stamp = format!("{}T{}Z", "20250102", "030405");
        let hex = format!("{}{}", "0123456789abcdef", "0123456789abcdef");
        for text in [
            format!("Session: {uuid}"),
            format!("{uuid}.jsonl"),
            format!("x-{}-y", uuid.to_uppercase()),
            format!("matrix-{stamp}-seats"),
            format!("thread {hex}"),
        ] {
            assert!(found_in(Class::Id, &text), "{text}");
        }
        assert!(!found_in(Class::Id, "00000000-0000-4000-8000-000000000001"));
        assert!(!found_in(Class::Id, "matrix-20260101T000000Z-seats"));
        assert!(!found_in(
            Class::Id,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/ws"
        ));
        assert!(!found_in(Class::Id, "0xcbf29ce484222325000000"));
    }

    #[test]
    fn an_email_address_of_any_domain_is_found() {
        for text in [
            format!("{}@{}.{}", "someone", "example", "org"),
            format!("{}@{}.{}", "someone", "gmail", "com"),
            format!("{}@{}.{}.{}", "a.b+c", "mail", "example", "co"),
            format!("mail me ({}@{}.{})", "x-y", "z", "io"),
        ] {
            assert!(found_in(Class::Email, &text), "{text}");
        }
        for clean in [
            "m-3 answered @worker.",
            "ask @chief.",
            "x @ y",
            "git@host:path",
            "@a.b",
        ] {
            assert!(!found_in(Class::Email, clean), "{clean}");
        }
    }

    #[test]
    fn the_prefix_of_a_key_or_a_token_is_found() {
        for text in [
            format!("{}{}", "sk-", "abcdefghijklmnop0123"),
            format!("{}{}", "ghp_", "abcdefghijklmnop0123"),
            format!("{}{}", "xoxb-", "0123456789-abcdefghij"),
            format!("{}{}", "eyJ", "hbGciOiJIUzI1NiIsInR5cCI6"),
            format!("{}{}", "AKIA", "ABCDEFGHIJKLMNOP"),
            format!(
                "Authorization: {} {}",
                "Bearer", "abcdefghijklmnopqrstuvwxyz0123"
            ),
        ] {
            assert!(found_in(Class::Key, &text), "{text}");
        }
        for clean in [
            "task-add-something-else-long",
            "the sk- prefix",
            "disk-usage-of-the-box",
        ] {
            assert!(!found_in(Class::Key, clean), "{clean}");
        }
    }

    /// The reviewer's own experiments, and every shape of a figure the scrub holds to a synthetic value:
    /// a clean row with one digit changed is found; so is a reset that is not one of the two phrases.
    #[test]
    fn a_usage_context_credit_quota_or_reset_figure_that_is_not_the_scrubs_is_found() {
        for row in CLEAN_FIGURES {
            let other = contaminated(row);
            assert_ne!(&other, row);
            assert!(found_in(Class::Figure, &other), "{other}");
        }
        for text in [
            // A credit row of other figures, and a reset of another time and day.
            format!("  Credits:             {OTHER_CREDITS} credits"),
            format!(
                "  Weekly limit:        [██████████░░░░░░░░░░] 55% left (resets {OTHER_RESET})"
            ),
            format!("  Xxx · 555% remaining (resets {OTHER_RESET})"),
            format!("SWE-2 High   Context: {} tokens (5%)", other_context()),
            format!(
                "  Context window:      {OTHER_LEFT}% left ({OTHER_USED}K used / {OTHER_TOTAL}K)"
            ),
            format!("Balance ${OTHER_SUM}"),
            format!("{OTHER_COUNT} credits"),
        ] {
            assert!(found_in(Class::Figure, &text), "{text}");
        }
        assert!(!found_in(Class::Figure, "the limit resets daily"));
        assert!(!found_in(
            Class::Figure,
            "Your next response will use more tokens"
        ));
        assert!(!found_in(Class::Figure, "Context compacted · 1s"));
    }

    /// A figure whose label is not on its line, which no rule of labels sees: a unit makes it one.
    #[test]
    fn a_figure_with_a_unit_is_found_with_its_label_on_another_line_or_none() {
        for text in [
            other_context(),
            format!("const LEFT: &str = \"{}{}\";", OTHER_LEFT, "%"),
            format!("R{}k", OTHER_COUNT.repeat(2)),
            format!("CH{}%", OTHER_USED),
            format!("{}M", OTHER_SUM),
        ] {
            assert!(found_in(Class::Figure, &text), "{text}");
        }
        // The scrub's own, zero, a word that goes on, a number that is part of a longer one.
        for clean in [
            "55k / 555k",
            "5.5M",
            "0%",
            "55%",
            "R55k",
            "CH55.5%",
            "5k8",
            "x2kb",
            "1.2.3%",
        ] {
            let found: Vec<_> = scan(clean, ALL)
                .into_iter()
                .filter(|found| found.why.starts_with("a figure with a unit"))
                .collect();
            let wanted = ["5k8", "x2kb", "1.2.3%"].contains(&clean);
            assert!(found.is_empty() || wanted, "{clean}: {found:?}");
        }
    }

    #[test]
    fn a_plan_name_is_found_where_the_synthetic_one_belongs() {
        for plan in PLANS {
            let header = format!("▝▜██████▀  Sonnet 5.5 with low effort · Claude {plan}");
            assert!(found_in(Class::Plan, &header), "{header}");
            let prose = format!("a Claude {} subscription", plan.to_lowercase());
            assert!(found_in(Class::Plan, &prose), "{prose}");
            let usage = "  Xxx · 555% remaining (resets in 12h 34m)".replace("Xxx", plan);
            assert!(found_in(Class::Plan, &usage), "{usage}");
        }
        assert!(!found_in(Class::Plan, "Claude Code v2.1.310"));
        assert!(!found_in(
            Class::Plan,
            "  Xxx · 555% remaining (resets in 12h 34m)"
        ));
        assert!(!found_in(
            Class::Plan,
            "SWE-2 Max   See usage and cost: /session-stats"
        ));
    }

    #[test]
    fn an_account_row_that_is_not_redacted_is_found() {
        for text in [
            format!("  {}:             {}", "Account", "someone"),
            format!("  {}: {}", "Email", "x"),
            format!("  {}: {}", "Organization", "Some Org"),
            format!("  {}: {}", "Login method", "Max Account"),
            format!("  {}:             {}", "Account", "<redacted> and more"),
        ] {
            assert!(found_in(Class::Account, &text), "{text}");
        }
        assert!(!found_in(
            Class::Account,
            "  Account:             <redacted>"
        ));
        assert!(!found_in(
            Class::Account,
            "const ACCOUNT_LABELS: &[&str] = &["
        ));
    }

    /// A scan says the line and the class, so a failure points at the row.
    #[test]
    fn a_scan_says_the_line_and_the_class_of_what_it_found() {
        let text = format!("fine\n  Credits:             {OTHER_CREDITS} credits\nfine");
        let found = scan(&text, ALL);
        assert!(!found.is_empty(), "{found:?}");
        assert!(
            found
                .iter()
                .all(|found| (found.line, found.class) == (2, Class::Figure)),
            "{found:?}"
        );
        assert!(scan(&text, &[Class::Name]).is_empty());
    }
}
