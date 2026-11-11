//! The paragraphs of the role texts that depend on the shell a window's commands are typed
//! in, from `skill/core/shells.md`: how a text is passed to `cf` (a heredoc in bash, zsh and
//! Git Bash; a file in PowerShell, which has no heredoc and reads `$`, the backtick and a
//! quote its own way), and how a removal whose path a variable makes is written. The shell is
//! the one of the window's `cf` word ([`CfWord::shell`]), so a text for a window in PowerShell
//! (Codex's and Muse's on Windows) is taught what that shell runs, and every other, what it was.

use cf_proto::cf_word::Shell;

use super::{member, RoleError};

/// `skill/core/shells.md`.
const SHELLS: &str = include_str!("../../../../skill/core/shells.md");

/// The part of `shells.md` marked `<!-- shell: slot -->`, as the text of a role holds it
/// where it names `{{slot}}`: its last line ending is the text's own, `{{cf}}` is still in it.
pub(super) fn paragraph(shell: Shell, slot: &str) -> Result<&'static str, RoleError> {
    part(SHELLS, shell, slot)
}

fn part(shells: &'static str, shell: Shell, slot: &str) -> Result<&'static str, RoleError> {
    let name = name(shell);
    let (_, parts) = member::split(shells);
    parts
        .into_iter()
        .find(|(part_shell, part_slot, _)| *part_shell == name && *part_slot == slot)
        .map(|(_, _, text)| text.trim_end_matches('\n'))
        .ok_or_else(|| RoleError::MissingShell {
            shell: name.to_owned(),
            slot: slot.to_owned(),
        })
}

/// The word `shells.md` marks a shell's parts with.
fn name(shell: Shell) -> &'static str {
    match shell {
        Shell::Posix => "posix",
        Shell::PowerShell => "powershell",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "<!-- about\nthis -->\n\
<!-- posix: input -->\nthe heredoc\n\
<!-- powershell: input -->\nthe file\n\n\
<!-- posix: only_posix -->\nbash\n";

    #[test]
    fn a_part_is_the_text_under_its_marker_without_its_last_line_ending() {
        assert_eq!(part(SAMPLE, Shell::Posix, "input").unwrap(), "the heredoc");
        assert_eq!(
            part(SAMPLE, Shell::PowerShell, "input").unwrap(),
            "the file"
        );
        assert_eq!(part(SAMPLE, Shell::Posix, "only_posix").unwrap(), "bash");
    }

    #[test]
    fn a_slot_a_shell_does_not_have_is_named() {
        assert_eq!(
            part(SAMPLE, Shell::PowerShell, "only_posix")
                .unwrap_err()
                .to_string(),
            "shells.md has no only_posix for the powershell shell"
        );
    }

    #[test]
    fn every_slot_the_texts_name_is_in_the_file_for_each_shell() {
        for shell in [Shell::Posix, Shell::PowerShell] {
            for slot in ["chief_input", "member_input", "removal"] {
                let text = paragraph(shell, slot).unwrap();
                assert!(
                    !text.is_empty() && !text.ends_with('\n'),
                    "{shell:?} {slot}"
                );
            }
        }
    }
}
