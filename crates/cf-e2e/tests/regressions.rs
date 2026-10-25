//! The regressions: one case for each bug the owner hit in real use, reproduced
//! as close to its real shape as the stand-ins allow, on the rig (the native
//! daemon and the real pane host, with stand-in agents in real terminals), and
//! asserting what the owner should have seen.
//!
//! The mechanism tests (the rig's, and each crate's own) say how a fix works. These
//! say what the owner saw, so a fix that passes those and fails these has missed the
//! bug. Each case is shown to fail on the version it was seen on, by the plant that
//! removes the fix (`tests/plants/daemon/`); where the cause was in the owner's
//! machine alone, its doc says so and the case is the closest shape of it.
//!
//! A bug found in real use is added here the same way, as a module of its own in
//! `regressions/` named for the report it answers, whose doc says:
//!
//! - the date and the version the owner saw it on;
//! - what the owner saw;
//! - the issue in the brain (consensflow-sme) that holds the report;
//!
//! and whose cases each assert what the owner should have seen. The cases take
//! turns, as the rig's do ([`cf_e2e::serial`]), and run wherever the rig's do; a
//! case whose bug Windows cannot have is Unix's.
//!
//!     cargo test -p cf-e2e --test regressions        (`npm run test:regressions`)

// The cases are in `tests/regressions/`: cargo looks for the modules of a test's
// root file beside it, where each would be a suite of its own.
// The stand-in's command is a shell of a Unix machine.
#[path = "regressions/a_brief_could_not_be_edited_once_added.rs"]
mod a_brief_could_not_be_edited_once_added;
#[path = "regressions/a_builds_daemon_held_a_workers_task.rs"]
mod a_builds_daemon_held_a_workers_task;
#[path = "regressions/a_codex_chief_opened_again_after_a_restart_read_starting_for_good.rs"]
mod a_codex_chief_opened_again_after_a_restart_read_starting_for_good;
#[cfg(unix)]
#[path = "regressions/a_command_that_goes_on_reporting_held_a_devin_workers_result.rs"]
mod a_command_that_goes_on_reporting_held_a_devin_workers_result;
#[path = "regressions/a_command_that_never_exits_held_a_workers_result.rs"]
mod a_command_that_never_exits_held_a_workers_result;
#[path = "regressions/a_compact_typed_into_an_idle_chief_held_a_workers_result.rs"]
mod a_compact_typed_into_an_idle_chief_held_a_workers_result;
#[path = "regressions/a_compaction_written_beside_an_answer_tore_its_line.rs"]
mod a_compaction_written_beside_an_answer_tore_its_line;
#[path = "regressions/a_follow_up_could_not_be_queued_behind_its_windows_own_work.rs"]
mod a_follow_up_could_not_be_queued_behind_its_windows_own_work;
#[path = "regressions/a_job_that_still_ran_was_taken_for_ended.rs"]
mod a_job_that_still_ran_was_taken_for_ended;
#[path = "regressions/a_pause_a_hung_worker_ignored_stranded_its_task.rs"]
mod a_pause_a_hung_worker_ignored_stranded_its_task;
#[path = "regressions/a_result_delivered_twice_to_a_devin_chief.rs"]
mod a_result_delivered_twice_to_a_devin_chief;
#[path = "regressions/a_result_pasted_again_after_its_window_went.rs"]
mod a_result_pasted_again_after_its_window_went;
#[path = "regressions/a_resumed_devin_worker_skipped_every_call.rs"]
mod a_resumed_devin_worker_skipped_every_call;
#[path = "regressions/a_switched_chief_was_sent_to_answer_a_question_on_a_cancelled_task.rs"]
mod a_switched_chief_was_sent_to_answer_a_question_on_a_cancelled_task;
#[path = "regressions/a_task_stuck_working_after_its_window_was_resumed.rs"]
mod a_task_stuck_working_after_its_window_was_resumed;
#[path = "regressions/a_task_waiting_on_a_cancelled_task_stayed_blocked_for_good.rs"]
mod a_task_waiting_on_a_cancelled_task_stayed_blocked_for_good;
#[path = "regressions/a_task_waiting_on_a_job_that_never_ends_was_finished_by_the_chief.rs"]
mod a_task_waiting_on_a_job_that_never_ends_was_finished_by_the_chief;
#[path = "regressions/a_trust_question_after_git_init_stopped_every_launch.rs"]
mod a_trust_question_after_git_init_stopped_every_launch;
#[path = "regressions/a_worker_that_waited_on_its_run_with_monitor_lost_it.rs"]
mod a_worker_that_waited_on_its_run_with_monitor_lost_it;
#[path = "regressions/a_workers_still_going_was_given_as_its_result.rs"]
mod a_workers_still_going_was_given_as_its_result;
// The stand-in's status line is a `bash`.
#[cfg(unix)]
#[path = "regressions/claude_status_line_taken_for_background_work.rs"]
mod claude_status_line_taken_for_background_work;
// The stand-in Codex is a script, so Unix's; and the Finder's PATH is a Mac's.
#[cfg(unix)]
#[path = "regressions/codex_in_homebrew_taken_for_not_installed.rs"]
mod codex_in_homebrew_taken_for_not_installed;
// The stand-in Codex is a script, so Unix's: Windows has no window of it to resume.
#[cfg(unix)]
#[path = "regressions/codex_role_on_resume.rs"]
mod codex_role_on_resume;
#[path = "regressions/devin_notes_twice.rs"]
mod devin_notes_twice;
#[path = "regressions/devin_pause.rs"]
mod devin_pause;
#[path = "regressions/devin_window_closed_mid_task.rs"]
mod devin_window_closed_mid_task;
#[path = "regressions/follow_up_refused_behind_a_waiting_follow_up.rs"]
mod follow_up_refused_behind_a_waiting_follow_up;
#[path = "regressions/idle_compaction_read_as_a_cut_turn.rs"]
mod idle_compaction_read_as_a_cut_turn;
#[path = "regressions/keys_cleared_with_escape_held_every_delivery.rs"]
mod keys_cleared_with_escape_held_every_delivery;
// Windows keeps the limit the report documents (a job object has no process
// groups or parents to look at), so there is nothing there to assert.
#[cfg(unix)]
#[path = "regressions/mcp_servers_held_a_workers_result.rs"]
mod mcp_servers_held_a_workers_result;
#[path = "regressions/opencode_updated_itself_to_2.rs"]
mod opencode_updated_itself_to_2;
#[path = "regressions/results_that_began_with_the_workers_progress.rs"]
mod results_that_began_with_the_workers_progress;
#[path = "regressions/resume_of_a_chief_with_no_turn.rs"]
mod resume_of_a_chief_with_no_turn;
#[path = "regressions/resume_of_a_turn_the_daemon_never_copied.rs"]
mod resume_of_a_turn_the_daemon_never_copied;
#[path = "regressions/rows_a_paste_carried_pasted_again_after_a_restart.rs"]
mod rows_a_paste_carried_pasted_again_after_a_restart;
#[path = "regressions/tasks_left_paused_by_a_close_and_resume.rs"]
mod tasks_left_paused_by_a_close_and_resume;
#[path = "regressions/tell_refused_for_a_queued_task.rs"]
mod tell_refused_for_a_queued_task;
#[cfg(unix)]
#[path = "regressions/whole_history_across_a_compaction_and_a_switch.rs"]
mod whole_history_across_a_compaction_and_a_switch;

use std::time::Duration;

use cf_e2e::rig::Config;

/// What a case ends with: nothing, or why it could not be run. A check that does
/// not hold is a panic, which is the case's failure. The error a case returns is
/// how the rigs it started learn that it failed.
type Outcome = cf_e2e::Outcome;

/// The stand-in for a window's harness program, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The stand-in for Devin, which the windows of Devin's agents run.
const FAKE_DEVIN: &str = env!("CARGO_BIN_EXE_fake-devin");

/// The stand-in for the `opencode` of a machine whose OpenCode has updated itself
/// to 2, which the daemon asks its version of before it opens a window on it.
const FAKE_OPENCODE_2: &str = env!("CARGO_BIN_EXE_fake-opencode-2");

/// The stand-in for Codex, which the windows of Codex's agents run.
const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

/// The configuration of a rig whose windows run the stand-in agent, to add
/// variables to. Its case is judged by what it returns, so a rig it drops unclosed
/// says nothing of a case that passed.
fn config() -> Config {
    Config::new(FAKE_AGENT).judged_by_outcome()
}

/// [`config`], and the stand-in Codex for the windows of Codex's agents.
fn config_with_codex() -> Config {
    config().codex(FAKE_CODEX)
}

/// `n` seconds.
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}
