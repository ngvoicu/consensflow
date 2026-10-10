//! `cf's verbs outside a window`: a verb of the native `cf` is given its
//! arguments whole. The Windows `.cmd` this replaced ran through cmd.exe, which
//! ends a command at its first line break: a reviewer's 6,250-character
//! question reached the chief as its first line, 528 characters, and `cf` said
//! it was asked (2026-10-03). The native `cf` runs here outside any window (a
//! window's token makes `cf` the board), and `cf agent add --description` keeps
//! the text it is given in the roster's file, so the roster is what shows what
//! the verb was given.

use cf_e2e::{cf, files, ScratchHome};
use regex::Regex;
use serde_json::Value;
use std::time::Duration;

use crate::Outcome;

/// Line breaks, quotes, a variable cmd.exe would expand, its operators,
/// diacritics.
const TEXT: &str = "Întrebarea 1: verific și versiunea în engleză?\n\
                    He said \"only the Romanian one\" & left; 100% sure | %PATH% ^ !x!\n\
                    \n\
                    PLOP-6142";

/// The description the roster keeps for the agent called `name`.
fn described(home: &ScratchHome, name: &str) -> cf_e2e::Result<Option<String>> {
    let roster: Value = serde_json::from_str(&files::read_string(
        &home.consensflow().join("agents.json"),
    )?)
    .map_err(|source| cf_e2e::Error::Json {
        text: "agents.json".to_owned(),
        source,
    })?;
    Ok(roster["agents"]
        .as_array()
        .and_then(|agents| agents.iter().find(|row| row["id"] == name))
        .and_then(|row| row["description"].as_str())
        .map(str::to_owned))
}

#[test]
fn are_given_every_argument_whole_line_breaks_quotes_and_and_diacritics() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf([
        "agent",
        "add",
        "whole",
        "--harness",
        "claude",
        "--model",
        "a-model",
        "--description",
        TEXT,
    ])?;
    assert_eq!(ran.code, Some(0), "{}", ran.stderr);
    assert_eq!(ran.stdout, "whole  claude  a-model\n");
    assert_eq!(described(&home, "whole")?.as_deref(), Some(TEXT));
    Ok(())
}

#[test]
fn end_as_the_verb_ends_its_words_on_stderr_and_its_exit_code() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf(["agent", "add", "lonely"])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert_eq!(ran.stdout, "");
    assert!(
        Regex::new(r"^cf: lonely needs --harness and --model")?.is_match(&ran.stderr),
        "{}",
        ran.stderr
    );
    Ok(())
}

// Codex runs its commands in PowerShell, and Claude Code has a PowerShell
// tool: both ran `& '…\cf.cmd' ask $q` with $q read from a file.
#[test]
#[cfg_attr(not(windows), ignore = "PowerShell is Windows'")]
fn take_a_many_line_argument_whole_from_powershell() -> Outcome {
    let home = ScratchHome::new()?;
    // Windows PowerShell 5.1 gives a native command an argument's own double
    // quotes bare, so this one has none.
    let text = TEXT.replace('"', "");
    let question = home.root().join("question.md");
    files::write(&question, &text)?;
    let ran = home
        .command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "$q = Get-Content -Raw -Encoding UTF8 '{}'; & '{}' agent add shell --harness claude --model a-model --description $q",
            question.display(),
            cf::binary()?.display()
        ))
        .finding_programs()
        // A cold Windows PowerShell on a CI runner can take more than the
        // usual limit just to start.
        .limit(Duration::from_secs(180))
        .run()?;
    assert_eq!(ran.code, Some(0), "{ran}");
    assert_eq!(described(&home, "shell")?.as_deref(), Some(text.as_str()));
    Ok(())
}
