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
//! the human's Resume of what a start suspended (`resume`), the conversation its
//! harness holds nothing of, which a window opened again does not resume
//! (`resumable`), and the arrival window counted in the time a window is at
//! rest, so that a record that is late while its window is in a turn is waited
//! for, as a message its harness says its own queue holds is (`slow_records`).

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
mod closing;
mod decided_results;
mod deleted_sessions;
mod deliveries;
mod exit_unsettled;
mod fixtures;
mod follow_ups;
mod interrupts;
mod lanes;
mod launch_stops;
mod launch_wait;
mod looks;
mod no_adapter;
mod not_ready;
mod out_of_quota;
mod participant_leaves;
mod pause_notes;
mod pending_agents;
mod queued_tells;
mod quota;
mod receipts;
mod restart;
mod resumable;
mod resume;
mod review;
mod role_change;
mod separate_callbacks;
mod servers;
mod session_windows;
mod sessions;
mod several_roles;
mod slow_records;
mod specs;
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
mod trace_untold;
mod traces;
mod turns_at_seams;
mod undeliverable;
mod unreadable_agents;
mod unsent;
mod unstarted;
mod waiting_notes;
mod wake_ups;
mod window_closes;
mod window_takes_long;
mod window_traces;
mod windows;
mod withdrawn;
mod work_in_flight;
mod work_in_flight_launches;
mod work_in_flight_pastes;
