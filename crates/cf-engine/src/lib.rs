//! The engine of ConsensFlow's daemon (`src/core/`), in the parts that are
//! text alone: how a message reads in its recipient's pane
//! ([`delivery_text`]), what a chief the human switched in is first told and
//! can read of the chiefs before it ([`handoff`]), and the instructions each
//! role's window starts with ([`roles`]).
//!
//! Nothing here touches a window, the ledger or a file the daemon owns: the
//! ledger's rows come in as `cf_proto::ledger`'s views, and what a text is
//! made of that is not in them (a message by its id, a role's card) is handed
//! in.

#![forbid(unsafe_code)]

pub mod delivery_text;
pub mod handoff;
pub mod roles;
