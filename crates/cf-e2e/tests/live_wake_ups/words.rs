//! What a run is told, and the limits it holds the daemon to: the interval of
//! the calls, the words the human edits them to, and what the chief is typed to
//! keep it in a turn longer than the interval.

use cf_e2e::live::Who;

/// The interval the calls are turned on at: a minute, the least there is. No
/// variable or switch shortens it, here or anywhere.
pub const EVERY_MS: i64 = 60_000;

/// How long after the calls were turned on (or their words edited) the daemon
/// has to queue the wake-up that is due: the interval, the chief's turn to be
/// over if it was in one, and the pass that finds it.
pub const FIRST_BY_MS: i64 = 150_000;

/// The quiet stretch a chief that did nothing on the first wake-up is watched
/// for: two intervals and a half, in which no second wake-up may come.
pub const QUIET_MS: u64 = 150_000;

/// The words the human edits the calls to: plain text of two lines with quotes in
/// it, which must arrive in the chief's window as they were written.
pub const EDITED: &str =
    "Is anything left on the board or in your plan? If so, carry on; if not, say \"nothing left\" and stop.\nAnswer in one line.";

/// What the chief answers when it is typed [`busy`].
pub const BUSY_DONE: &str = "BUSY_DONE";

/// What the daemon's task for a staff member says, which the stand-in worker
/// does at once and answers.
pub const PROBE: &str = "Reply with exactly: PROBE_DONE";

/// How long the command that keeps a real chief busy runs, in milliseconds: well
/// past an interval, so that a wake-up falls due in the middle of the turn.
const BUSY_MS: u64 = 100_000;

/// The mark that is in the busy command's own command line and nowhere else of
/// the machine's, by which what it leaves is found and ended.
pub const MARK: &str = "cf-wake-up-long-command";

/// What the human types to the chief to keep it in a turn for [`BUSY_MS`]: the
/// stand-in works that long on its own line; a real chief is told to run a
/// command that long and to answer when it is over.
pub fn busy(who: Who) -> String {
    match who {
        Who::StandIn => format!("SLEEP {} Reply with exactly: {BUSY_DONE}", BUSY_MS / 1000),
        Who::Harness(_) => format!(
            "Run this exact command with your shell tool, wait for it to finish, and then reply with exactly one line: {BUSY_DONE}\n{}",
            command()
        ),
    }
}

/// The command a real chief is told to run: Node waits for [`BUSY_MS`], and the
/// mark is a word it is given and does nothing with.
pub fn command() -> String {
    format!("node -e \"setTimeout(() => {{}}, {BUSY_MS})\" {MARK}")
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::Harness;

    use super::*;

    #[test]
    fn the_interval_is_the_least_a_wake_up_may_wait_and_the_quiet_stretch_two_and_a_half_of_it() {
        assert_eq!(EVERY_MS, 60_000);
        assert_eq!(QUIET_MS * 2, 5 * EVERY_MS as u64);
        assert_eq!(
            FIRST_BY_MS, 150_000,
            "an interval and a chief's turn and a pass"
        );
    }

    #[test]
    fn the_edited_words_have_two_lines_and_quotes_and_nothing_a_terminal_would_change() {
        assert_eq!(EDITED.lines().count(), 2);
        assert!(EDITED.contains('"'));
        assert!(EDITED.is_ascii());
    }

    #[test]
    fn the_stand_in_works_for_the_busy_time_and_a_real_chief_is_told_to_run_a_command_that_long() {
        assert_eq!(
            busy(Who::StandIn),
            "SLEEP 100 Reply with exactly: BUSY_DONE"
        );
        let told = busy(Who::Harness(Harness::Claude));
        assert!(told.starts_with("Run this exact command with your shell tool"));
        assert!(told.ends_with(&command()));
        assert_eq!(
            command(),
            "node -e \"setTimeout(() => {}, 100000)\" cf-wake-up-long-command"
        );
        assert_eq!(BUSY_MS, 100_000, "a minute and the margin of a pass");
    }
}
