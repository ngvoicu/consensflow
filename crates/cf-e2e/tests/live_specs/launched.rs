//! What a window was launched with, judged from its `pane.open` and from the
//! files that request names: the role text it was given (where its harness is
//! given it), the Specmint skill that text names, and which copy of the skill that
//! is. The chief of a project with specs on is told of them, and every other
//! window, a chief's with specs off and every member's, is told nothing.

use cf_e2e::live::cut;
use regex::Regex;
use serde_json::Value;

/// What the section of specs in a chief's text begins with.
const SECTION: &str = "## This project's specs";

/// How many characters of a launch a report shows before the word it found.
const MARGIN: usize = 50;

/// The words Codex's developer instructions say ConsensFlow's role begins with,
/// after what Codex itself gives a window (its own instructions, which hold
/// whatever the human has set up: a skill of Specmint, a note on it).
const CODEX_ROLE: &str = "\n\nYour ConsensFlow role is ";

/// The part of a window's role text that is ConsensFlow's: all of it, but for
/// Codex's, which follows what Codex gives every window.
pub fn ours(text: &str) -> &str {
    text.rfind(CODEX_ROLE).map_or(text, |at| &text[at..])
}

/// What a window's text says of specs where it should say nothing: the
/// section's title, or the skill's name.
pub fn told_nothing_of_specs(role: &str) -> Result<(), String> {
    for said in [SECTION, "Specmint"] {
        if role.contains(said) {
            return Err(format!("the text has {said:?} where specs are not on"));
        }
    }
    Ok(())
}

/// Whether the text has the section of specs.
pub fn has_the_section(role: &str) -> bool {
    role.contains(SECTION)
}

/// The file of the skill the text names: the `SKILL.md` its section says it is at.
pub fn skill_named_in(role: &str) -> Option<String> {
    Regex::new(r"its SKILL\.md is at (.+?), and the files it names are beside it")
        .ok()?
        .captures(role)
        .map(|found| found[1].to_owned())
}

/// The folder the text says specs are kept in, where it says `.specs`.
pub fn folder_named_in(role: &str) -> Option<String> {
    Regex::new(r"Where Specmint says `\.specs`, it means `([^`]+)`")
        .ok()?
        .captures(role)
        .map(|found| found[1].to_owned())
}

/// The `name` a `SKILL.md` gives its skill in its frontmatter.
pub fn skill_called(file_text: &str) -> Option<String> {
    let frontmatter = file_text.strip_prefix("---")?.split("\n---").next()?;
    frontmatter.lines().find_map(|line| {
        let name = line.trim().strip_prefix("name:")?;
        Some(name.trim().trim_matches(['"', '\'']).to_owned())
    })
}

/// Where a window was given its role text, by what its `pane.open` says: a file
/// (Claude's, OpenCode's and Devin's), or the text itself (Pi's, and Codex's as
/// the developer's instructions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleText {
    File(String),
    Inline(String),
}

/// How the window of `frame` (a `pane.open` request) was given its role: Claude
/// appends a file to its system prompt, Pi appends the text, Codex is given it as
/// developer instructions (a JSON string in a setting of its command line), Devin
/// reads the file its environment names, and OpenCode has it as the last of the
/// instructions its configuration lists.
pub fn role_of(frame: &Value) -> Option<RoleText> {
    let argv: Vec<&str> = frame["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let after = |flag: &str| {
        argv.iter()
            .position(|word| *word == flag)
            .and_then(|at| argv.get(at + 1))
            .map(|word| (*word).to_owned())
    };
    if let Some(file) = after("--append-system-prompt-file") {
        return Some(RoleText::File(file));
    }
    if let Some(text) = after("--append-system-prompt") {
        return Some(RoleText::Inline(text));
    }
    if let Some(setting) = argv
        .iter()
        .find_map(|word| word.strip_prefix("developer_instructions="))
    {
        let text: String = serde_json::from_str(setting).ok()?;
        return Some(RoleText::Inline(text));
    }
    if let Some(file) = frame["env"]["CF_DEVIN_ROLE_FILE"].as_str() {
        return Some(RoleText::File(file.to_owned()));
    }
    let configuration: Value =
        serde_json::from_str(frame["env"]["OPENCODE_CONFIG_CONTENT"].as_str()?).ok()?;
    let file = configuration["instructions"].as_array()?.last()?.as_str()?;
    Some(RoleText::File(file.to_owned()))
}

/// That no word of the window's launch (its command line and its environment)
/// names a Specmint skill: a window of no specs is given none beside its role.
/// (Codex's developer instructions are no word of this: they hold what Codex
/// itself says of the human's own skills.)
pub fn carries_no_skill(frame: &Value) -> Result<(), String> {
    let argv = frame["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|word| !word.starts_with("developer_instructions="))
        .map(|word| ("command line", word.to_owned()));
    let env = frame["env"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| Some(("environment", format!("{name}={}", value.as_str()?))));
    for (part, word) in argv.chain(env) {
        if let Some(at) = word.find("specmint") {
            let from = word[..at]
                .char_indices()
                .rev()
                .nth(MARGIN)
                .map_or(0, |(start, _)| start);
            return Err(format!(
                "the window's {part} names a Specmint skill: {}",
                cut(&word[from..], MARGIN * 3)
            ));
        }
    }
    Ok(())
}

/// The path as the product compares paths: with slashes, as the section of specs
/// writes it on every system.
fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// Which copy of the skill the chief was given, judged: the file its text names is
/// there and is the skill (`frontmatter`, the `name` of its `SKILL.md`, is
/// `specmint-<variant>`), and it is the harness's own where the harness has the
/// variant installed (`installed`, the file its adapter says) and ConsensFlow's
/// own, under `home`, where it has not. What the run says of it, in a line.
pub fn which_copy(
    variant: &str,
    named: Option<&str>,
    frontmatter: Option<&str>,
    installed: Option<&str>,
    home: &str,
) -> Result<String, String> {
    let Some(file) = named else {
        return Err("the chief's text names no SKILL.md for the skill".to_owned());
    };
    let skill = format!("specmint-{variant}");
    if frontmatter != Some(skill.as_str()) {
        return Err(format!(
            "{file} is no copy of {skill}: its frontmatter names {frontmatter:?}"
        ));
    }
    match installed {
        Some(own) if slashed(own) == slashed(file) => {
            Ok(format!("the harness's own copy of {skill}, at {file}"))
        }
        Some(own) => Err(format!(
            "the harness has {skill} installed at {own}, and the chief's text names {file}"
        )),
        None if slashed(file).starts_with(&slashed(home)) => {
            Ok(format!("ConsensFlow's copy of {skill}, at {file}"))
        }
        None => Err(format!(
            "the harness has no {skill} installed, and the chief's text names {file}, which is not under {home}"
        )),
    }
}

#[cfg(test)]
#[path = "launched/tests.rs"]
mod tests;
