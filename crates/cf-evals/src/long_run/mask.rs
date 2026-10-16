//! Hiding what looks like a key or a token in what a long run keeps and says: a
//! window's screen, a record's lines, a command line, a ledger row, a failure's
//! words. The rules are the product's (`crates/cf-engine/src/shown/mask.rs`, which a
//! crate of the evals may not link), read by shape and not by meaning, and two more.
//! The value of a secret name written in quotes is hidden whole, spaces and all
//! (`"password": "correct horse battery staple"`), where the product hides its first
//! word. And the run's own names are shown: the handle of each participant of its
//! project and the id of each of their windows ([`Names`], read from the ledger and the
//! windows), which the product's rule of 24 characters would hide (the dry run's report
//! named nobody). Only those: a value shaped like a name (`abc123-def456-ghi789-jkl012`)
//! that is none of the run's is hidden as any token is.
//!
//! What is hidden, each by `[masked]` in its place:
//! - the user and password of a URL (`https://user:password@host`);
//! - what follows `Bearer`, eight characters or more; and what follows `Basic` when
//!   it is base64's (a digit in it, a capital that does not begin it, or ending in
//!   `=`);
//! - the value of a name that says it is secret, `=` or `:` between them: after a
//!   name with `password`, `passwd`, `pwd`, `secret` or `credential` in it, six
//!   characters or more; after a name with `key` or `token` in it, a value of eight
//!   characters or more with a digit in it, or sixteen or more; in quotes, the whole
//!   of what the quotes hold, a quote escaped in it (`\"`) and all, and in quotes
//!   escaped themselves, as JSON writes a value that holds them, its escapes read to
//!   its real end (`{"error":"password=\"hunter2\\\"keepme\""}`, a refusal said in a
//!   failure); and a value whose quote never closes, to the end of the text (a command
//!   the pane host cut at 512 characters);
//! - a token of a kind with a known prefix (`sk-`, `ghp_` and its kin, `github_pat_`,
//!   `glpat-`, `xoxb-`, `AKIA…`, `AIza…`, `npm_`, `hf_`, `ya29.`, `SG.`, a JWT's
//!   `eyJ…`);
//! - any run of 24 letters, digits, `_` and `-` with a letter and a digit in it that
//!   is none of the run's names, and any run of 40 of base64's own characters with
//!   the same.
//!
//! A text is masked whole before it is cut short ([`cut`]): a secret whose end is past
//! the cut is no secret's shape once it is cut, and would be kept as it was.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::ledger::Ledger;
use super::look::Look;

/// What a hidden secret is replaced with.
pub const MASK: &str = "[masked]";

/// The run's own names, which are shown: each participant's handle and each of their
/// windows' ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names(BTreeSet<String>);

impl Names {
    /// The names a look shows: the handles of its project's participants, the id of
    /// the window each has (`p<project>-<handle>`), and the ids of the windows it found
    /// open, exited or left.
    pub fn of(look: &Look) -> Self {
        let mut names = Self::default();
        names.add(look);
        names
    }

    /// The names a ledger holds: the handles of its project's participants, and the
    /// id of the window each has (`p<project>-<handle>`).
    pub fn of_ledger(ledger: &Ledger) -> Self {
        let mut names = Self::default();
        let project = ledger.project.as_ref().map(|project| project.id);
        for someone in &ledger.participants {
            names.0.insert(someone.handle.clone());
            if let Some(project) = project {
                names.0.insert(format!("p{project}-{}", someone.handle));
            }
        }
        names
    }

    /// The names `look` shows, added.
    pub fn add(&mut self, look: &Look) {
        self.0.extend(Self::of_ledger(&look.ledger).0);
        let windows = &look.windows;
        let panes = windows
            .open
            .iter()
            .map(|window| &window.pane)
            .chain(windows.exits.iter().map(|exit| &exit.pane))
            .chain(windows.strangers.iter().map(|stranger| &stranger.pane))
            .chain(windows.unresolved.iter().map(|left| &left.pane));
        self.0.extend(panes.cloned());
    }

    /// `name` is one of the run's from now: a file it keeps, by its name or a folder of
    /// its path.
    pub fn insert(&mut self, name: impl Into<String>) {
        self.0.insert(name.into());
    }

    /// `other`'s names, added.
    pub fn extend(&mut self, other: &Names) {
        self.0.extend(other.0.iter().cloned());
    }

    /// Whether `found` is one of the run's names.
    fn holds(&self, found: &str) -> bool {
        self.0.contains(found)
    }
}

/// A pattern, the groups of it that may hold the secret (0 for all of it; the first
/// that matched is the one), whether what it matched is one, and whether a run's name
/// it matched whole is shown.
struct Rule {
    pattern: Regex,
    secret: &'static [usize],
    is_secret: fn(&str) -> bool,
    names_shown: bool,
}

/// The patterns are constants of this file, and a test builds each: a mistake in one
/// fails that test, and no input reaches it.
#[allow(clippy::expect_used)]
fn rule(pattern: &str, secret: &'static [usize], is_secret: fn(&str) -> bool) -> Rule {
    Rule {
        pattern: Regex::new(pattern).expect("a pattern of the masker"),
        secret,
        is_secret,
        names_shown: false,
    }
}

fn always(_: &str) -> bool {
    true
}

fn has_a_letter_and_a_digit(found: &str) -> bool {
    found.chars().any(|c| c.is_ascii_alphabetic()) && found.chars().any(|c| c.is_ascii_digit())
}

/// The value of a name that says it is secret, but not the scheme of a header, nor
/// what is hidden already.
fn a_value(found: &str) -> bool {
    !found.starts_with(MASK)
        && !found.eq_ignore_ascii_case("bearer")
        && !found.eq_ignore_ascii_case("basic")
}

/// What `Basic` carries: base64 of `user:password`, which is not a word.
fn base64_credentials(found: &str) -> bool {
    has_a_letter_and_a_digit(found)
        || found.ends_with('=')
        || found.chars().skip(1).any(|c| c.is_ascii_uppercase())
}

/// The value of a name that is a word as well (`key`, `token`): one that is long,
/// or has a digit in it, not the word that says why it failed.
fn a_long_or_numbered_value(found: &str) -> bool {
    a_value(found)
        && (found.chars().count() >= 16
            || (found.chars().count() >= 8 && found.chars().any(|c| c.is_ascii_digit())))
}

/// What follows the secret word of a name to its value: the rest of the name, its
/// quote (plain, or escaped as JSON escapes it), and `=` or `:`.
const NAMED: &str = r#"[A-Za-z0-9_.-]*(?:\\?["'])?\s*[:=]\s*"#;

/// One character of a value in quotes that JSON escaped (`\"…\"`), as JSON wrote it:
/// a plain one, an escape of JSON's (`\n`, `é`), or an escape the value had of
/// its own, written with its backslash escaped (`\\\"` is the value's `\"`, which
/// does not end it). A line break is a character of the value like any other.
const ESCAPED: &str = r#"(?:[^"\\]|\\[^"\\]|\\\\(?:\\(?s:.)|[^"\\]))"#;

/// One character of a value in double quotes, or in single quotes: a plain one, or
/// one its backslash escapes (an escaped quote is part of the value). A line break is
/// a character of the value like any other: a value in quotes ends at its quote.
const DOUBLE: &str = r#"(?:[^"\\]|\\(?s:.))"#;
const SINGLE: &str = r"(?:[^'\\]|\\(?s:.))";

/// A secret name's value of `least` characters or more, each shape a group of its
/// own: in quotes JSON escaped, in double quotes, in single quotes (each to its
/// closing quote, whatever lines it spans); then in one of those quotes that never
/// closes, which is the value to the end of the text (a backslash the text was cut
/// after with it); then bare.
fn quoted(least: usize) -> String {
    format!(
        concat!(
            r#"(?:\\"({escaped}{{{least},}})\\""#,
            r#"|"({double}{{{least},}})""#,
            r"|'({single}{{{least},}})'",
            r#"|\\"({escaped}+)\\?\z"#,
            r#"|"({double}+)\\?\z"#,
            r"|'({single}+)\\?\z",
            r#"|([^\s"',;]{{{least},}}))"#,
        ),
        escaped = ESCAPED,
        double = DOUBLE,
        single = SINGLE,
        least = least,
    )
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule(
            r"(?i)\b[a-z][a-z0-9+.-]*://([^\s/@:]+:[^\s/@]+)@",
            &[1],
            always,
        ),
        rule(r"(?i)\bbearer\s+([A-Za-z0-9._~+/=-]{8,})", &[1], always),
        rule(
            r"(?i)\bbasic\s+([A-Za-z0-9._~+/=-]{8,})",
            &[1],
            base64_credentials,
        ),
        // NAME=value, NAME: value, "name": "value", "name": "a value with spaces",
        // "name": "a \"quoted\" value", each of them inside a JSON string (its quotes
        // and its escapes escaped again), and a quoted value whose closing quote never
        // comes (a command the pane host cut at 512 characters).
        rule(
            &format!(
                r"(?i)\b[A-Za-z0-9_.-]*(?:passw(?:or)?d|pwd|secret|credential){NAMED}{}",
                quoted(6)
            ),
            &[1, 2, 3, 4, 5, 6, 7],
            a_value,
        ),
        rule(
            &format!(r"(?i)\b[A-Za-z0-9_.-]*(?:key|token){NAMED}{}", quoted(8)),
            &[1, 2, 3, 4, 5, 6, 7],
            a_long_or_numbered_value,
        ),
        rule(
            concat!(
                r"\b(?:(?:sk|pk|rk)[-_][A-Za-z0-9_-]{12,}",
                r"|gh[pousr]_[A-Za-z0-9]{20,}",
                r"|github_pat_[A-Za-z0-9_]{20,}",
                r"|glpat-[A-Za-z0-9_-]{16,}",
                r"|xox[abprs]-[A-Za-z0-9-]{10,}",
                r"|(?:AKIA|ASIA)[A-Z0-9]{16}",
                r"|AIza[A-Za-z0-9_-]{30,}",
                r"|npm_[A-Za-z0-9]{30,}",
                r"|hf_[A-Za-z0-9]{30,}",
                r"|ya29\.[A-Za-z0-9._-]{20,}",
                r"|SG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}",
                r"|eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*)",
            ),
            &[0],
            always,
        ),
        Rule {
            names_shown: true,
            ..rule(r"[A-Za-z0-9_-]{24,}", &[0], has_a_letter_and_a_digit)
        },
        rule(r"[A-Za-z0-9+/=]{40,}", &[0], has_a_letter_and_a_digit),
    ]
});

/// `text` with what looks like a key or a token hidden, the run's own `names` shown.
pub fn mask(text: &str, names: &Names) -> String {
    let mut text = text.to_owned();
    for rule in RULES.iter() {
        text = rule
            .pattern
            .replace_all(&text, |found: &Captures<'_>| hide(rule, found, names))
            .into_owned();
    }
    text
}

/// The first `most` UTF-16 units of `text` once it is masked whole: what a line of
/// evidence, a record's entry or the timeline keeps of a long text.
pub fn cut(text: &str, most: usize, names: &Names) -> String {
    crate::js::first(&mask(text, names), most)
}

/// How wide a row is before it is taken for one the terminal wrapped, where it is
/// the screen's widest: every window of a run is as wide (the daemon's `pane.open`
/// asks no size, the pane host's default is 80 by 24, and nothing resizes a window).
const WRAPS_AT: usize = 80;

/// The rows of a screen masked as one text: a secret's name may end one row and its
/// value begin the next (`DB_PASSWORD=` / `"correct horse battery staple"`), and a
/// terminal wraps a long line at its width, a token run across two rows. The rows are
/// joined as they were drawn (a row as wide as the screen's widest, of [`WRAPS_AT`]
/// characters or more, runs on into the next as the terminal wrapped it; any other
/// ends its line), what is to be hidden is found in the whole, and each row keeps its
/// own part of it: `[masked]` where a secret begins, nothing of it in the rows it runs
/// on into. Then each row is masked as a text of its own too. Rows joined may hide a
/// little more than either would alone, and never less.
pub fn screen(rows: &[String], names: &Names) -> Vec<String> {
    let widest = rows
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0);
    let mut joined = String::new();
    let mut places = Vec::new();
    for row in rows {
        let start = joined.len();
        joined.push_str(row);
        places.push(start..joined.len());
        if !(widest >= WRAPS_AT && row.chars().count() == widest) {
            joined.push('\n');
        }
    }
    let hidden = spans(&joined, names);
    places
        .into_iter()
        .map(|place| {
            let mut shown = String::new();
            let mut at = place.start;
            for span in hidden
                .iter()
                .filter(|span| span.start < place.end && span.end > place.start)
            {
                shown.push_str(&joined[at..span.start.max(at)]);
                if span.start >= place.start {
                    shown.push_str(MASK);
                }
                at = span.end.min(place.end);
            }
            shown.push_str(&joined[at..place.end]);
            mask(&shown, names)
        })
        .collect()
}

/// Where in `text` each rule finds a secret to hide (the run's `names` shown), every
/// rule read against the whole text: the stretches, in order, those that meet made one.
fn spans(text: &str, names: &Names) -> Vec<std::ops::Range<usize>> {
    let mut found: Vec<std::ops::Range<usize>> = Vec::new();
    for rule in RULES.iter() {
        for matched in rule.pattern.captures_iter(text) {
            let Some(secret) = rule.secret.iter().find_map(|group| matched.get(*group)) else {
                continue;
            };
            if (rule.is_secret)(secret.as_str())
                && !(rule.names_shown && names.holds(secret.as_str()))
            {
                found.push(secret.range());
            }
        }
    }
    found.sort_by_key(|span| span.start);
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for span in found {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }
    merged
}

/// What replaces a match of `rule`: the match with its secret hidden, or the match
/// as it was where it holds none (or is one of the run's `names`, for a rule that
/// shows them).
fn hide(rule: &Rule, found: &Captures<'_>, names: &Names) -> String {
    let whole = &found[0];
    let (Some(all), Some(secret)) = (
        found.get(0),
        rule.secret.iter().find_map(|group| found.get(*group)),
    ) else {
        return whole.to_owned();
    };
    if !(rule.is_secret)(secret.as_str()) || (rule.names_shown && names.holds(secret.as_str())) {
        return whole.to_owned();
    }
    let (before, after) = (secret.start() - all.start(), secret.end() - all.start());
    format!("{}{MASK}{}", &whole[..before], &whole[after..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::long_run::ledger::{Participant, Project};

    /// `text` masked with no name of a run's.
    fn masked(text: &str) -> String {
        mask(text, &Names::default())
    }

    #[test]
    fn every_rule_is_built() {
        assert_eq!(RULES.len(), 8);
    }

    #[test]
    fn what_the_product_hides_is_hidden_here_and_what_it_leaves_is_left() {
        for (text, hidden) in [
            (
                "git clone https://alice:hunter2pass@github.com/x/y.git failed",
                "git clone https://[masked]@github.com/x/y.git failed",
            ),
            (
                "Authorization: Bearer abc123.def456.ghi789",
                "Authorization: Bearer [masked]",
            ),
            ("Basic QWxhZGRpbjpvcGVu", "Basic [masked]"),
            ("OPENAI_API_KEY=abcd1234efgh", "OPENAI_API_KEY=[masked]"),
            (
                r#"{"password": "hunter2hunter", "user": "alice"}"#,
                r#"{"password": "[masked]", "user": "alice"}"#,
            ),
            ("db_pwd = hunter2", "db_pwd = [masked]"),
            ("token=abcd1234", "token=[masked]"),
            ("API key: abcdefghijklmnop", "API key: [masked]"),
            (
                "the credential was ghp_abcdefghijklmnopqrstuvwxyz0123456789 here",
                "the credential was [masked] here",
            ),
            ("id 0123456789abcdef0123456789abcdef ok", "id [masked] ok"),
            (
                "secret wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY!",
                "secret [masked]!",
            ),
        ] {
            assert_eq!(masked(text), hidden, "{text}");
        }
        for prose in [
            "basic configuration is missing",
            "Invalid token: expired",
            "API key: missing",
            "token=abcdefgh",
            "authorized: yes",
            "ConsensFlowSupervisorStartupFailedError",
            "Error: connect ECONNREFUSED 127.0.0.1:8080",
            "No API key found for the selected model. Use /login to log into a provider.",
        ] {
            assert_eq!(masked(prose), prose, "{prose}");
        }
    }

    #[test]
    fn a_secret_value_in_quotes_is_hidden_whole_spaces_and_all() {
        assert_eq!(
            masked(r#"{"password": "correct horse battery staple", "user": "alice"}"#),
            r#"{"password": "[masked]", "user": "alice"}"#
        );
        assert_eq!(
            masked("export DB_SECRET='open sesame 42'"),
            "export DB_SECRET='[masked]'"
        );
        assert_eq!(
            masked(r#"api_key: "sk live abcd 1234""#),
            r#"api_key: "[masked]""#
        );
        // A quoted word that is no secret's value stays.
        assert_eq!(masked(r#"token: "expired""#), r#"token: "expired""#);
    }

    #[test]
    fn a_secret_value_with_its_quotes_escaped_is_hidden_whole() {
        for (text, hidden) in [
            // A quote escaped inside the value does not end it.
            (
                r#"{"password": "hunter2\"keepme123"}"#,
                r#"{"password": "[masked]"}"#,
            ),
            // A failure that holds the daemon's refusal as JSON: the value's quotes
            // are escaped in the string that holds it.
            (
                r#"{"error":"password=\"correct horse battery staple\""}"#,
                r#"{"error":"password=\"[masked]\""}"#,
            ),
            (
                r#"project.resume: {"ok":false,"error":"the token=\"abcd1234 efgh5678\" expired"}"#,
                r#"project.resume: {"ok":false,"error":"the token=\"[masked]\" expired"}"#,
            ),
            // JSON held in a JSON string: the name's quotes are escaped too.
            (
                r#""{\"password\": \"correct horse\"}""#,
                r#""{\"password\": \"[masked]\"}""#,
            ),
            // A quote escaped in the value, inside a JSON string: the value's `\"` is
            // written `\\\"`, and does not end it (Astra's string).
            (
                r#"{"error":"password=\"hunter2\\\"keepme123\""}"#,
                r#"{"error":"password=\"[masked]\""}"#,
            ),
        ] {
            assert_eq!(masked(text), hidden, "{text}");
        }
        // Sol's: a refusal of the daemon's whose error holds the password with a quote
        // escaped in it, said as the page's answer is said in a failure.
        let error = r#"password="hunter2\"keepme123""#;
        let said = format!(
            "project.resume: {}",
            serde_json::json!({ "ok": false, "error": error })
        );
        assert!(said.contains(r#"hunter2\\\"keepme123"#), "{said}");
        let hidden = masked(&said);
        assert!(!hidden.contains("keepme123"), "{hidden}");
        assert!(!masked(&hidden).contains("keepme123"));
        assert!(
            hidden.contains(r#""error":"password=\"[masked]\"""#),
            "{hidden}"
        );
    }

    #[test]
    fn a_secret_value_in_quotes_that_holds_a_line_break_is_hidden_whole() {
        for (text, hidden) in [
            (
                "DB_PASSWORD=\"correct horse\nbattery staple\"",
                "DB_PASSWORD=\"[masked]\"",
            ),
            // In a failure of many lines, what follows the value is kept.
            (
                "the daemon said:\npassword: 'correct horse\nbattery staple' was refused\nand went on",
                "the daemon said:\npassword: '[masked]' was refused\nand went on",
            ),
            // In quotes JSON escaped.
            (
                "{\"error\":\"password=\\\"correct horse\nbattery staple\\\" refused\"}",
                "{\"error\":\"password=\\\"[masked]\\\" refused\"}",
            ),
        ] {
            assert_eq!(masked(text), hidden, "{text}");
        }
    }

    #[test]
    fn a_screen_is_masked_as_one_text_its_wrapped_rows_joined() {
        // The terminal wrapped the line right after the secret's name.
        let rows: Vec<String> = [
            "$ export DB_PASSWORD=",
            "\"correct horse battery staple\"",
            "$ ",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            screen(&rows, &Names::default()),
            ["$ export DB_PASSWORD=", "\"[masked]\"", "$ "]
        );
        // A token the wrap cut in two, the first row as wide as the terminal: hidden in
        // both rows.
        let wrapped = format!("{}deploy with ghp_abcdefghij", " ".repeat(54));
        assert_eq!(wrapped.chars().count(), 80);
        let rows = vec![wrapped.clone(), "klmnopqrstuvwxyz0123456789 now".to_owned()];
        assert_eq!(
            screen(&rows, &Names::default()),
            [
                format!("{}deploy with [masked]", " ".repeat(54)),
                " now".to_owned()
            ]
        );
        // A row that ends its line is no wrap: the next row's words are its own.
        let rows: Vec<String> = [
            "$ export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "Allow this edit? (y/n)",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            screen(&rows, &Names::default()),
            ["$ export GITHUB_TOKEN=[masked]", "Allow this edit? (y/n)"]
        );
        // Rows with nothing to hide are as they were, and each row's own secret too.
        let rows: Vec<String> = ["all 61 tests pass", "token=abcd1234efgh done"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            screen(&rows, &Names::default()),
            ["all 61 tests pass", "token=[masked] done"]
        );
        // The run's own names on a screen are shown.
        let mut names = Names::default();
        names.insert("p1-long-claude-1-olive-valley");
        let rows = vec!["window p1-long-claude-1-olive-valley exited".to_owned()];
        assert_eq!(screen(&rows, &names), rows);
    }

    #[test]
    fn a_secret_value_whose_quote_never_closes_is_hidden_to_the_end_of_the_text() {
        // The pane host cut the command at 512 characters, in the middle of the value.
        for (text, hidden) in [
            (
                r#"node deploy.mjs --env password="correct horse battery stap…"#,
                r#"node deploy.mjs --env password="[masked]"#,
            ),
            (
                "export API_TOKEN='abcd1234 efgh5678 ijkl…",
                "export API_TOKEN='[masked]",
            ),
            // In a JSON string, cut after the backslash of an escape.
            (
                r#"{"error":"secret=\"correct horse\\\"batt\"#,
                r#"{"error":"secret=\"[masked]\"#,
            ),
            // To the end of the text, its lines and all.
            (
                "db_password = \"correct horse\nbattery staple",
                "db_password = \"[masked]",
            ),
        ] {
            assert_eq!(masked(text), hidden, "{text}");
        }
        // A short value that closes is no secret's, and stays.
        assert_eq!(
            masked(r#"token: "expired" for now"#),
            r#"token: "expired" for now"#
        );
    }

    #[test]
    fn a_text_is_masked_whole_before_it_is_cut() {
        // The secret's closing quote is the 201st character: cut first, it is no
        // quoted value, and the whole of it would be kept.
        let text = format!(
            "{}password=\"correct horse battery staple\" and more",
            "a ".repeat(81)
        );
        assert_eq!(text.find("\" and").map(|at| at + 1), Some(201));
        let kept = cut(&text, 200, &Names::default());
        assert!(!kept.contains("horse"), "{kept}");
        assert!(kept.ends_with("password=\"[masked]\" and more"), "{kept}");
        assert_eq!(cut("a long line of words", 6, &Names::default()), "a long");
    }

    /// The names of a project 1 with the chief and the dry run's two workers.
    fn names() -> Names {
        let mut look = Look::default();
        look.ledger.project = Some(Project {
            id: 1,
            name: "ledgerline".to_owned(),
            state: "open".to_owned(),
        });
        for (id, handle) in [
            (2, "chief"),
            (3, "long-claude-1-olive-valley"),
            (4, "long-devin-1-olive-summit"),
        ] {
            look.ledger.participants.push(Participant {
                id,
                handle: handle.to_owned(),
                role: "worker".to_owned(),
                harness: None,
                agent: None,
            });
        }
        Names::of(&look)
    }

    #[test]
    fn the_runs_own_handles_and_window_ids_are_shown_and_nothing_else_shaped_like_them() {
        let names = names();
        let mut look = Look::default();
        look.ledger.project = Some(Project {
            id: 1,
            name: "ledgerline".to_owned(),
            state: "open".to_owned(),
        });
        look.ledger.participants.push(Participant {
            id: 3,
            handle: "long-claude-1-olive-valley".to_owned(),
            role: "worker".to_owned(),
            harness: None,
            agent: None,
        });
        assert!(Names::of_ledger(&look.ledger).holds("p1-long-claude-1-olive-valley"));
        // The dry run's own lines, which the product's rule of 24 characters hid whole.
        for named in [
            "@long-claude-1-olive-valley's window p1-long-claude-1-olive-valley killed (process 44417)",
            "the app quit while @long-devin-1-olive-summit was at work on T-19",
        ] {
            assert_eq!(mask(named, &names), named, "{named}");
        }
        // A session credential with the shape of a name, which names nobody of the run.
        for (text, hidden) in [
            (
                "Set-Cookie: session=abc123-def456-ghi789-jkl012",
                "Set-Cookie: session=[masked]",
            ),
            (
                "@long-claude-2-amber-harbor is no participant of this run",
                "@[masked] is no participant of this run",
            ),
            (
                "session 123e4567-e89b-12d3-a456-426614174000 resumed",
                "session [masked] resumed",
            ),
        ] {
            assert_eq!(mask(text, &names), hidden, "{text}");
        }
    }
}
