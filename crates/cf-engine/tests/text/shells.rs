//! What the role texts teach about the shell a window's commands are typed in (T-1435): a
//! window in PowerShell (Codex's and Muse's on Windows) is not taught a heredoc, which
//! PowerShell has none of, nor a removal written as bash writes it, and is taught `--file`;
//! a window in bash, zsh or Git Bash is taught what it always was, to the byte (the goldens
//! of `tests/goldens/text.json` hold those, played for `Shell::Posix`).

use cf_base::env::Env;
use cf_engine::roles::{role_instructions, ProjectSpecs, StaffRow};
use cf_proto::agents::WorkTier;
use cf_proto::cf_word::{CfWord, Shell};
use cf_proto::specs::Variant;

use super::support::staff_row;

const ROLES: [&str; 6] = [
    "chief", "worker", "advisor", "reviewer", "tester", "designer",
];

/// A path with a space, which PowerShell runs after an `&`; one without, which both take as
/// the word it is; and none, the word `cf`.
const SPACED: &str = "C:/Program Files/ConsensFlow/bin/cf.exe";
const PLAIN: &str = "C:/Users/AGV/consensflow/bin/cf.exe";

fn staff() -> Vec<StaffRow> {
    vec![staff_row(
        "zeus",
        &["worker", "reviewer"],
        WorkTier::Standard,
    )]
}

fn text(role: &str, cf: &CfWord, specs: Option<&ProjectSpecs<'_>>) -> String {
    role_instructions(&Env::default(), role, &staff(), cf, specs).unwrap()
}

fn specs() -> ProjectSpecs<'static> {
    ProjectSpecs {
        variant: Variant::Core,
        folder: ".specs",
        skill: "/home/me/.consensflow/skills/specmint-core/SKILL.md",
    }
}

/// What a PowerShell window is taught of passing a text to `cf`, for the chief and for a
/// member, and of a removal, with this window's word for `cf` in it.
fn chief_input(word: &str) -> String {
    format!(
        "Any \"…\" can be `--file brief.md` instead: the text then comes from that file,\n\
         as written. Write a brief that holds backticks, `$` or quotes to it with your\n\
         file tool, never with `>` (Windows PowerShell 5.1 writes UTF-16) or a pipe into\n\
         `-` (it loses non-ASCII there). In double quotes PowerShell runs `$( )`, `$name`\n\
         and the backtick before ConsensFlow sees them, and 5.1 splits the text at an\n\
         inner `\"`; to put a value in, write it out:\n\n    {word} task add --tier standard --file brief.md\n"
    )
}

fn member_input(word: &str) -> String {
    format!(
        "Any \"…\" can be `--file ask.md` instead, the text in that file, written with your\n\
         file tool and taken as written: `{word} ask --file ask.md`. In double quotes\n\
         PowerShell runs `$( )`, `$name` and the backtick.\n"
    )
}

const REMOVAL: &str = "Nobody watches this window, and a harness may stop for a human even with\n\
every permission granted: Claude Code does on any removal (`rm`, `rm -f`,\n\
`rmdir`) whose path a variable makes, and no permission lets it through.\n\
Write a removal's paths out in full.\n";

fn powershell_words() -> Vec<CfWord> {
    [SPACED, PLAIN, ""]
        .into_iter()
        .map(|path| CfWord::new(path, Shell::PowerShell))
        .collect()
}

/// PowerShell has no heredoc: its window is taught `--file`, with no `<<'` anywhere, whether
/// its `cf` is a path with a space (a call after `&`), a path without or no path at all.
#[test]
fn a_powershell_window_is_taught_a_file_and_no_heredoc() {
    for cf in powershell_words() {
        let word = cf.as_str();
        for role in ROLES {
            for specs in [None, Some(&specs())] {
                let text = text(role, &cf, specs);
                let which = format!("the {role}'s, {word}");
                assert!(!text.contains("<<'"), "{which}: a heredoc");
                assert!(!text.contains("heredoc"), "{which}: a heredoc");
                assert!(!text.contains("{{"), "{which}: a slot left");
                if role == "chief" {
                    assert!(text.contains(&chief_input(word)), "{which}\n{text}");
                    assert!(!text.contains(" - <<"), "{which}");
                } else {
                    assert!(text.contains(&member_input(word)), "{which}\n{text}");
                    assert!(
                        !text.contains(&chief_input(word)),
                        "{which}: the chief's paragraph"
                    );
                }
            }
        }
    }
}

/// `"$R/$b"` and `"${R:?}/${b:?}"` are bash: in PowerShell `${R:?}` is a variable of a drive
/// named `R` and `$R/$b` a division. Its member is told the rule in words (T-1435).
#[test]
fn a_powershell_member_is_told_the_removal_rule_in_words_and_no_bash() {
    for cf in powershell_words() {
        for role in ROLES.into_iter().filter(|role| *role != "chief") {
            let text = text(role, &cf, None);
            assert!(text.contains(REMOVAL), "the {role}'s\n{text}");
            for bash in ["$R/$b", "${R:?}", "${b:?}"] {
                assert!(!text.contains(bash), "the {role}'s says {bash}");
            }
        }
    }
}

/// A bash-like window is taught the heredoc and the removal it was: these are the words,
/// and the goldens hold the rest of the text to the byte.
#[test]
fn a_bash_like_window_keeps_its_heredoc_and_its_removal_example() {
    for path in [SPACED, PLAIN, ""] {
        let cf = CfWord::new(path, Shell::Posix);
        let word = cf.as_str();
        let chief = text("chief", &cf, None);
        assert!(
            chief.contains(&format!(
                "    {word} task add --tier standard - <<'BRIEF'\n"
            )),
            "{word}"
        );
        assert!(chief.contains("Write a long brief to a file with your file tool"));
        assert!(!chief.contains("(Windows PowerShell"), "{word}");
        for role in ROLES.into_iter().filter(|role| *role != "chief") {
            let text = text(role, &cf, None);
            assert!(
                text.contains(&format!(
                    "`{word} ask - <<'TEXT'`, the text, then a line `TEXT`."
                )),
                "the {role}'s, {word}"
            );
            assert!(
                text.contains("(`\"$R/$b\"`)") && text.contains("or as `\"${R:?}/${b:?}\"`."),
                "the {role}'s, {word}"
            );
        }
    }
}

/// The two texts of a window differ in these paragraphs and no other, and a PowerShell
/// window's is no longer than a bash-like window's with the same word: what is taught of the
/// shell is paid for by what it makes unneeded (T-1435, no text ends longer than it was).
#[test]
fn a_powershell_text_is_no_longer_than_the_bash_like_text_with_the_same_word() {
    for path in [PLAIN, ""] {
        let posix = CfWord::new(path, Shell::Posix);
        let powershell = CfWord::new(path, Shell::PowerShell);
        assert_eq!(posix.as_str(), powershell.as_str(), "the same word");
        for role in ROLES {
            for specs in [None, Some(&specs())] {
                let (posix, powershell) =
                    (text(role, &posix, specs), text(role, &powershell, specs));
                assert!(
                    powershell.len() <= posix.len(),
                    "the {role}'s, {path:?}: {} bytes against {}",
                    powershell.len(),
                    posix.len()
                );
                // The lines of one that the other lacks are those of the paragraphs, and no other:
                // the chief's input (9 lines of the bash text are not in the PowerShell text, 7
                // of the PowerShell text are not in the bash text) and, for a member, its input
                // (3 and 3) and its removal (2 and 2).
                let lines = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
                let (posix, powershell) = (lines(&posix), lines(&powershell));
                let lacks = |of: &[String], from: &[String]| {
                    from.iter().filter(|line| !of.contains(line)).count()
                };
                let counts = (lacks(&powershell, &posix), lacks(&posix, &powershell));
                let expected = if role == "chief" { (9, 7) } else { (5, 5) };
                assert_eq!(counts, expected, "the {role}'s, {path:?}");
            }
        }
    }
}
