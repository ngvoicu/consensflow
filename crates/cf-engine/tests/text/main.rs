//! What the engine says in text, held to Node: the goldens of
//! `tests/goldens/text.json` (`npm run goldens:engine`, from
//! `tests/goldens/engine/`) played row by row, and the JavaScript tests of
//! the same modules (`core-delivery-text`, `handoff`, `core-roles`, `skill`)
//! ported under their sentences.

// The goldens' own reading and the tests' own building: a failure there is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod delivery_text;
mod goldens_delivery;
mod goldens_handoff;
mod goldens_roles;
mod handoff;
mod roles;
mod skill;
mod support;
