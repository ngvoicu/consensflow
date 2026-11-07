//! What a window was launched with, judged from its `pane.open` and from the
//! files that request names: the role text it was given (where its harness is
//! given it), the Specmint skill that text names, and which copy of the skill that
//! is. The chief of a project with specs on is told of them, and every other
//! window, a chief's with specs off and every member's, is told nothing. And what a
//! Codex chief's own record holds after its Resume, which a launch's command line
//! cannot say: its thread keeps the instructions it began with, so the section of
//! specs is there only as a message ConsensFlow gave it.

use std::path::Path;

use cf_e2e::live::cut;
use cf_e2e::rollout;
use regex::Regex;
use serde_json::Value;

use crate::evidence::DEFAULT_FOLDER;

/// What the section of specs in a chief's text begins with.
const SECTION: &str = "## This project's specs";

/// What the message that gives a role again says of it, which no other note does:
/// the words that say it replaces the instructions the conversation began with.
const REPLACES: &str = "replace the ones you were given then";

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

/// The thread a window was opened on again: the word after `resume` on its
/// command line, in the `pane.open` of `frame`. None for a window that began one.
pub fn thread_resumed(frame: &Value) -> Option<String> {
    let argv: Vec<&str> = frame["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let at = argv.iter().position(|word| *word == "resume")?;
    argv.get(at + 1).map(|thread| (*thread).to_owned())
}

/// Whether a chief's own record (the rollout of its Codex thread, `record`) holds
/// the section of specs, and where, which the chief reads whatever its launch
/// said. A thread that began with specs off keeps the developer instructions of
/// that start, so the section is there as a message ConsensFlow gave it when the
/// chief was resumed, one that says it replaces the instructions the thread began
/// with; a thread that began with specs on has it in those.
pub fn told_in_the_record(record: &str) -> Result<String, String> {
    let developer = rollout::said_by(record, "developer");
    if developer.iter().any(|said| said.contains(SECTION)) {
        return Ok("in the developer instructions its thread began with".to_owned());
    }
    let user = rollout::said_by(record, "user");
    match user.iter().find(|said| said.contains(SECTION)) {
        Some(said) if said.contains(REPLACES) => Ok(
            "in a message of ConsensFlow's that replaces the instructions its thread began with"
                .to_owned(),
        ),
        Some(_) => Err(
            "a message of the thread's has the section, and does not say it replaces the instructions the thread began with"
                .to_owned(),
        ),
        None => Err(format!(
            "its record has no section of specs: {} developer message{} and {} user message{}",
            developer.len(),
            if developer.len() == 1 { "" } else { "s" },
            user.len(),
            if user.len() == 1 { "" } else { "s" },
        )),
    }
}

/// The file of the skill the text names: the `SKILL.md` its section says it is at.
pub fn skill_named_in(role: &str) -> Option<String> {
    Regex::new(r"its SKILL\.md is at (.+?), and the files it names are beside it")
        .ok()?
        .captures(role)
        .map(|found| found[1].to_owned())
}

/// The folder the text says the specs and their registry are in.
pub fn folder_named_in(role: &str) -> Option<String> {
    Regex::new(r"Your specs and their registry are in `([^`]+)`:")
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
/// (Claude's, Devin's, Codex's and Muse's), or the text itself (Pi's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleText {
    File(String),
    Inline(String),
}

/// How the window of `frame` (a `pane.open` request) was given its role: Claude
/// appends a file to its system prompt, Pi appends what its last
/// `--append-system-prompt` says, a text or (since Pi 1.4.0, which ConsensFlow gives the
/// role by a file so that no process of the machine reads it) a file's path
/// ([`RoleText::Inline`] holds either; [`text_of`] tells them apart), Codex's broker
/// and Devin read the file their environment names, and Muse's SessionStart hook
/// (`-c hooks=…`) hands it the file its `cf hook muse-session` names ([`muse_role_file`]).
pub fn role_of(frame: &Value) -> Option<RoleText> {
    let argv: Vec<&str> = frame["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    // The last: the file Pi appends of the user's own (`APPEND_SYSTEM.md`) comes first, and
    // the role after it.
    let after = |flag: &str| {
        argv.iter()
            .rposition(|word| *word == flag)
            .and_then(|at| argv.get(at + 1))
            .map(|word| (*word).to_owned())
    };
    if let Some(file) = after("--append-system-prompt-file") {
        return Some(RoleText::File(file));
    }
    if let Some(text) = after("--append-system-prompt") {
        return Some(RoleText::Inline(text));
    }
    for named in ["CF_CODEX_ROLE_FILE", "CF_DEVIN_ROLE_FILE"] {
        if let Some(file) = frame["env"][named].as_str() {
            return Some(RoleText::File(file.to_owned()));
        }
    }
    argv.iter()
        .find_map(|word| muse_role_file(word))
        .map(RoleText::File)
}

/// The role file a Muse window's `-c hooks=<json>` word gives its SessionStart hook: the
/// word after `hook muse-session` in the hook's POSIX `command`, which quotes each word in
/// single quotes (`cf_harness::muse::launch`), or in its Windows `commandWindows`, which
/// quotes none. None for any other word (T-954: `live specs muse` read no role of Muse's).
pub fn muse_role_file(word: &str) -> Option<String> {
    let hooks: Value = serde_json::from_str(word.strip_prefix("hooks=")?).ok()?;
    let lines = hooks["SessionStart"]
        .as_array()?
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
        .flat_map(|hook| [(&hook["command"], true), (&hook["commandWindows"], false)])
        .filter_map(|(line, posix)| Some((line.as_str()?, posix)));
    for (line, posix) in lines {
        let words = if posix {
            shell_words(line)
        } else {
            line.split_whitespace().map(str::to_owned).collect()
        };
        if let Some(at) = words
            .windows(2)
            .position(|pair| pair[0] == "hook" && pair[1] == "muse-session")
        {
            return words.get(at + 2).cloned();
        }
    }
    None
}

/// The words a Pi window's `--append-system-prompt` says: the file the path names, where
/// it names one (Pi reads the text of a path that is a file, `resolvePromptInput`), else
/// the argument is the text.
pub fn text_of(appended: &str) -> std::result::Result<String, String> {
    if !appended.contains('\n') && Path::new(appended).is_file() {
        return std::fs::read_to_string(appended)
            .map_err(|failed| format!("the role text {appended} cannot be read: {failed}"));
    }
    Ok(appended.to_owned())
}

/// The words of a POSIX shell's line of plain and single-quoted words (`'it'\''s'` is
/// one word, `it's`): what a Muse hook's `command` is made of.
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        match character {
            '\'' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if !quoted => {
                if let Some(next) = characters.next() {
                    word.push(next);
                    started = true;
                }
            }
            blank if blank.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            other => {
                word.push(other);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// That no word of the window's launch (its command line and its environment)
/// names a Specmint skill: a window of no specs is given none beside its role.
pub fn carries_no_skill(frame: &Value) -> Result<(), String> {
    let argv = frame["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
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

/// That the skill the chief is given names the project's folder and no other: no file of
/// its folder `skill` (the folder of its `SKILL.md`, with its `references` and `assets`)
/// says `.specs` where the project keeps its specs in `folder`, and its `SKILL.md` says
/// `folder`. The skill's own files call `.specs` the place the specs and the registry are
/// in, and a chief that read both went by the skill (T-662, Pi on `docs/specs`). Where
/// the folder is `.specs` there is no other to name. What the run says of it, in a line.
pub fn names_the_folder_alone(skill: &Path, folder: &str) -> Result<String, String> {
    if folder == DEFAULT_FOLDER {
        return Ok(format!("{folder} is the folder the skill names"));
    }
    let mut named = false;
    let mut files = 0;
    let mut pending = vec![skill.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = std::fs::read_dir(&next)
            .map_err(|failed| format!("{} cannot be read: {failed}", next.display()))?;
        for entry in entries {
            let path = entry.map_err(|failed| failed.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|failed| format!("{} cannot be read: {failed}", path.display()))?;
            files += 1;
            if let Some(at) = text.find(DEFAULT_FOLDER) {
                return Err(format!(
                    "{} still says {DEFAULT_FOLDER}: {}",
                    path.display(),
                    cut(&text[at..], MARGIN * 3)
                ));
            }
            named |=
                path.file_name().is_some_and(|name| name == "SKILL.md") && text.contains(folder);
        }
    }
    named
        .then(|| {
            format!(
                "{files} files, none of them says {DEFAULT_FOLDER}, and its SKILL.md says {folder}"
            )
        })
        .ok_or_else(|| format!("its SKILL.md does not say {folder}"))
}

/// The path as the product compares paths: with slashes, as the section of specs
/// writes it on every system.
fn slashed(path: &str) -> String {
    path.replace('\\', "/")
}

/// Where ConsensFlow keeps its copies of Specmint, below its folder `home`: one
/// for each harness and variant, in no launch's folder, so that a chief's text
/// names the same file at every launch.
fn kept_below(home: &str) -> String {
    format!(
        "{}/integrations/specmint/",
        slashed(home).trim_end_matches('/')
    )
}

/// Which copy of the skill the chief was given, judged: the file its text names is
/// there and is the skill (`frontmatter`, the `name` of its `SKILL.md`, is
/// `specmint-<variant>`), and it is the harness's own where the harness has the
/// variant installed (`installed`, the file its adapter says) and ConsensFlow's
/// own where it has not: the one kept for every launch, below `home`, and not one
/// in the folder of the launch. What the run says of it, in a line.
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
        None if slashed(file).starts_with(&kept_below(home)) => {
            Ok(format!("ConsensFlow's copy of {skill}, at {file}"))
        }
        None => Err(format!(
            "the harness has no {skill} installed, and the chief's text names {file}, which is not in the folder ConsensFlow keeps its copies in ({})",
            kept_below(home)
        )),
    }
}

#[cfg(test)]
#[path = "launched/tests.rs"]
mod tests;
