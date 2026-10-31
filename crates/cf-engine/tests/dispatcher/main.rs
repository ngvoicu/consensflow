//! The dispatcher's tests (Node's dispatcher suite), ported under their
//! sentences, each held to the Node trace of the same test ([`traces`]). What
//! Node's dispatcher does not do has tests of its own, held to no trace: the
//! notes of a pause (`pause_notes`), and the receipt and stop redesign
//! (`carriers`, `launch_stops`, `receipts`, `stops` and `taken_back`), and what
//! a window that did not come up showed (`unstarted`), and the result a
//! decision withdraws before the chief is given it (`decided_results`), and the
//! notes of a task's wait that go when it moves on, and the word that follows a
//! hold (`waiting_notes`), and the result a turn's end is not while the work it
//! started still runs (`background`), and the same of the agents a turn left
//! running, which a window takes input beside (`pending_agents`), and the switch
//! after a chief's turn, which waits for them (`switching_agents`), and the time
//! a launch is told it has for its first message (`launch_wait`), and the
//! processes a harness keeps of its own, which are no work a turn left (`servers`),
//! and what a
//! window is given while the words that were for it wait at the human's gate
//! (`approval`), and the tester, which Node's staff never had (`testers`), and
//! the human's Resume of what a start suspended (`resume`), and of what a Close
//! suspended, whose windows are handed nothing meanwhile (`closed_given_nothing`), the conversation its
//! harness holds nothing of, which a window opened again does not resume
//! (`resumable`), and the arrival window counted in the time a window is at
//! rest, so that a record that is late while its window is in a turn is waited
//! for, as a message its harness says its own queue holds is (`slow_records`),
//! and the paste a window went with, which its harness's record may show it took
//! (`window_went_with_a_paste`), and a header quoted in a window's words, which is
//! no paste (`quoted_headers`), and the shapes a harness's record keeps a paste in
//! (`paste_shapes`), the quota wait that keeps a member's task with its
//! window whoever else is free (`quota_waits`), and an account's quota as every
//! window's on it, and what waits for a window out of quota said to the human
//! (`quota_accounts`), and a window whose turn is over, which no stop interrupts
//! or closes (`nothing_to_stop`), and a read of a window that fails or comes too late, which is
//! unknown, and a pass that leaves a window whose read hangs to it (`unknown_looks`).

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod adapters_failing_at_once;
mod agent_gone;
mod approval;
mod assigning;
mod background;
mod cancelled_task;
mod carriers;
mod chief_not_up;
mod chiefs;
mod closed_given_nothing;
mod closing;
mod compacting;
mod computer_use;
mod decided_results;
mod deleted_sessions;
mod deliveries;
mod exit_overdue;
mod exit_proof;
mod exit_unsettled;
mod failure_reasons;
mod final_answer;
mod first_delivered;
mod fixtures;
mod follow_ups;
mod interrupts;
mod lanes;
mod launch_stops;
mod launch_wait;
mod looks;
mod looks_finished;
mod lost_lines;
mod meeting_main;
mod moved_models;
mod no_adapter;
mod not_ready;
mod nothing_to_stop;
mod open_deadline;
mod out_of_quota;
mod own_steps;
mod pacing;
mod participant_leaves;
mod paste_shapes;
mod pause_notes;
mod paused_answers;
mod paused_left;
mod pending_agents;
mod queued_tells;
mod quota;
mod quota_accounts;
mod quota_waits;
mod quoted_headers;
mod read_in_the_inbox;
mod receipts;
mod restart;
mod resumable;
mod resume;
mod retired_agents;
mod review;
mod role_change;
mod schedules;
mod separate_callbacks;
mod servers;
mod session_windows;
mod sessions;
mod several_roles;
mod shared;
mod slow_records;
mod specs;
mod stale_box;
mod still_there;
mod stops;
mod switched_conversation;
mod switching;
mod switching_agents;
mod switching_deleted;
mod switching_handoffs;
mod switching_refused;
mod taken_back;
mod tasks;
mod testers;
mod the_dispatcher;
mod throws_early;
mod told_once;
mod trace_untold;
mod traces;
mod turn_end;
mod turns_at_seams;
mod unavailable_accounts;
mod undeliverable;
mod unfit;
mod unknown_looks;
mod unproven;
mod unreadable_agents;
mod unsent;
mod unstarted;
mod waiting_notes;
mod wake_ups;
mod window_closes;
mod window_takes_long;
mod window_traces;
mod window_went_with_a_paste;
mod windows;
mod withdrawn;
mod withheld_enter;
mod work_in_flight;
mod work_in_flight_launches;
mod work_in_flight_pastes;
