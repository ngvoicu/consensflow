//! The dispatcher's tests (`tests/core-dispatcher.test.mjs`), ported under
//! their sentences, each held to the Node trace of the same test
//! ([`traces`]).

// The tests' own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod agent_gone;
mod assigning;
mod chief_not_up;
mod chiefs;
mod closing;
mod deleted_sessions;
mod deliveries;
mod fixtures;
mod interrupts;
mod lanes;
mod looks;
mod no_adapter;
mod out_of_quota;
mod quota;
mod restart;
mod review;
mod session_windows;
mod sessions;
mod several_roles;
mod switching;
mod switching_deleted;
mod switching_handoffs;
mod switching_refused;
mod tasks;
mod the_dispatcher;
mod throws_early;
mod traces;
mod unreadable_agents;
mod windows;
mod work_in_flight;
mod work_in_flight_launches;
mod work_in_flight_pastes;
