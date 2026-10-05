//! Devin's adapter, as `tests/adapter-devin.test.mjs` holds Node's
//! (TEST-BDC-05, IMPL-BDC-07), each case under its sentence, and the cases of
//! `tests/role-skills.test.mjs` that load Devin's role: how a Devin window is
//! launched (`launches`), how it names the conversation it opened, how a message
//! reaches it, and what its own wire log and record say (`windows`). Each test
//! gets a throwaway home, Devin config folder and a stand-in `devin` on PATH.
//!
//! A window comes only from a prepare here, where Node's tests made up a
//! launch bag: a test of a window on a known conversation prepares it as
//! that conversation resumed. Node's launch ids were any filename-safe
//! word; here each is a uuid, as the engine mints one. Where Node injected
//! what its record said, a stand-in record says it here.

mod fixtures;
mod launches;
mod windows;
