//! The work the advisor of a project is given while the human has it paused: a
//! minute of it, so that its window is in the middle of its turn when the pause
//! comes. It has to be work that every model will do and that no role text
//! forbids. The first words for it were `sleep 60`, and an advisor on Pi refused
//! them: the role text of every member says never to wait in the shell
//! (`skill/core/staff.md`: "no `sleep`, …"). So the minute is a script of the
//! project's folder that does real work for as long as it lasts, a chain of
//! SHA-256 hashes, and the brief names the script and never the wait.

use std::path::Path;

use cf_e2e::{files, Result};

/// The script's file, in the project's folder.
pub const SCRIPT: &str = "minute.pl";

/// The script. It hashes for the seconds it is given, a minute when it is given
/// none, and says how many rounds it made. (`Time::HiRes`: perl's own `time` is
/// whole seconds, and a minute that ends at the next one may be a moment.)
const PROGRAM: &str = "\
use strict;
use warnings;
use Digest::SHA qw(sha256_hex);
use Time::HiRes qw(time);

# Real work for a minute: a chain of hashes, as long as the minute lasts.
my $seconds = shift // 60;
my $end = time() + $seconds;
my ($rounds, $digest) = (0, 'consensflow');
while (time() < $end) {
    $digest = sha256_hex($digest);
    $rounds++;
}
print \"$rounds rounds, last digest $digest\\n\";
";

/// Puts the script in the project's `folder`, which the advisor's window runs in.
pub fn write(folder: &Path) -> Result {
    files::write(&folder.join(SCRIPT), PROGRAM)
}

/// What the advisor is asked: to run the script, then to answer with `marker`.
/// The chief types it into `cf task add "…"`, so no character of it is one the
/// shell reads inside double quotes.
pub fn brief(marker: &str) -> String {
    format!("Run the shell command perl {SCRIPT}, then reply with exactly: {marker}")
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use cf_e2e::live;

    use super::*;

    /// The words the role text of a member says not to run in its shell: what is
    /// in backticks in the parenthesis after "Never wait for an answer in your shell".
    fn forbidden(role_text: &str) -> Vec<String> {
        // A role text is wrapped: its lines are one.
        let role_text = role_text.split_whitespace().collect::<Vec<_>>().join(" ");
        let after = role_text
            .split("Never wait for an answer in your shell")
            .nth(1)
            .unwrap_or_default();
        let parenthesis = after
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .map_or("", |(inside, _)| inside);
        parenthesis
            .split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn the_role_text_of_a_member_names_what_it_may_not_run_in_its_shell() {
        let text = "Questions go to the chief. Never wait for an answer\n\
                    in your shell (no `sleep`, no loop over `cf inbox`): it arrives later.";
        assert_eq!(forbidden(text), ["sleep", "cf inbox"]);
        assert!(forbidden("no such sentence (`sleep`)").is_empty());
        // The real one says `sleep`, which an advisor on Pi took at its word.
        let staff = std::fs::read_to_string(cf_e2e::checkout::path("skill/core/staff.md")).unwrap();
        assert!(forbidden(&staff).contains(&"sleep".to_owned()), "{staff}");
    }

    #[test]
    fn what_the_advisor_is_asked_to_do_is_nothing_a_role_text_forbids() {
        let staff = std::fs::read_to_string(cf_e2e::checkout::path("skill/core/staff.md")).unwrap();
        let asked = brief("ADVISOR_OK_P1");
        for word in forbidden(&staff) {
            for text in [asked.as_str(), PROGRAM] {
                assert!(
                    !text.to_lowercase().contains(&word),
                    "`{word}` is in what the advisor is given: {text}"
                );
            }
        }
        assert_eq!(
            asked,
            "Run the shell command perl minute.pl, then reply with exactly: ADVISOR_OK_P1"
        );
    }

    #[test]
    fn the_brief_holds_nothing_the_shell_reads_inside_the_double_quotes_the_chief_types_it_in() {
        let asked = brief("ADVISOR_OK_P12");
        for read_by_the_shell in ['$', '`', '"', '\\', '!'] {
            assert!(
                !asked.contains(read_by_the_shell),
                "{read_by_the_shell}: {asked}"
            );
        }
    }

    #[test]
    fn the_script_works_for_the_seconds_it_is_given_and_says_how_many_rounds_it_made() {
        // A machine with no perl has nothing to run it on: the live run needs one,
        // as the command that keeps the chief at work does.
        let Ok(perl) = live::command("perl") else {
            return;
        };
        let folder = tempfile::tempdir().unwrap();
        write(folder.path()).unwrap();
        let started = Instant::now();
        let ran = cf_e2e::process::Run::new(perl)
            .arg(folder.path().join(SCRIPT))
            .arg("1")
            .inheriting_env()
            .run()
            .unwrap();
        assert_eq!(ran.code, Some(0), "{ran}");
        assert!(started.elapsed().as_secs_f64() >= 1.0, "it worked a second");
        let (rounds, digest) = ran
            .stdout
            .trim()
            .split_once(" rounds, last digest ")
            .unwrap_or(("", ""));
        assert!(rounds.parse::<u64>().unwrap_or(0) > 0, "{}", ran.stdout);
        assert_eq!(digest.len(), 64, "{}", ran.stdout);
        // Given no seconds it is a minute: the number is the script's own.
        assert!(PROGRAM.contains("shift // 60"));
    }
}
