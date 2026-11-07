//! `cf's verbs outside a window`: a verb of the native `cf` is given its
//! arguments whole. The Windows `.cmd` this replaced ran through cmd.exe, which
//! ends a command at its first line break: a reviewer's 6,250-character
//! question reached the chief as its first line, 528 characters, and `cf` said
//! it was asked (2026-10-03). The native `cf` runs here outside any window (a
//! window's token makes `cf` the board), and a word that is no command is said back
//! whole, as JSON writes it (`cf: unknown command "…"`), so its error output is what
//! shows what `cf` was given.

use cf_e2e::{cf, files, ScratchHome};
use serde_json::Value;
use std::time::Duration;

use crate::Outcome;

/// Line breaks, quotes, a variable cmd.exe would expand, its operators,
/// diacritics.
const TEXT: &str = "Întrebarea 1: verific și versiunea în engleză?\n\
                    He said \"only the Romanian one\" & left; 100% sure | %PATH% ^ !x!\n\
                    \n\
                    PLOP-6142";

/// What `cf` says of `word`, given as its command: that it has none of that name, the word
/// whole, as JSON writes it.
fn unknown(word: &str) -> String {
    format!(
        "cf: unknown command {} — run `cf help`\n",
        Value::String(word.to_owned())
    )
}

#[test]
fn are_given_every_argument_whole_line_breaks_quotes_and_and_diacritics() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf([TEXT])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert_eq!(ran.stdout, "");
    assert_eq!(ran.stderr, unknown(TEXT));
    Ok(())
}

#[test]
fn end_as_the_verb_ends_its_words_on_stderr_and_its_exit_code() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf(["setup", "lonely"])?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ran.stderr,
        "cf: Unexpected argument 'lonely'. This command does not take positional arguments\n"
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
            "$q = Get-Content -Raw -Encoding UTF8 '{}'; & '{}' $q",
            question.display(),
            cf::binary()?.display()
        ))
        .finding_programs()
        // A cold Windows PowerShell on a CI runner can take more than the
        // usual limit just to start.
        .limit(Duration::from_secs(180))
        .run()?;
    assert_eq!(ran.code, Some(1), "{ran}");
    assert_eq!(ran.stderr, unknown(&text));
    Ok(())
}
