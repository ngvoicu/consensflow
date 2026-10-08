//! `cf setup`: prepares what the app prepares when it opens, the terminal
//! command and the extensions of Pi and OpenCode
//! (`cf_harness::prepare::prepare_app`), and says which harnesses it found and
//! how many agents there are.
//!
//! The order is Node's, and it is what the recording holds: the words are read
//! first (a word nobody asked for leaves the folder as it was), the
//! preparation comes next, and the roster last, so a file of agents that
//! cannot be read says so after the command was made and what was found was
//! said.

use std::io::Write;

use cf_base::args::{self, Positionals};
use cf_base::env::Env;
use cf_harness::prepare::prepare_app;

use super::{agents_saved, harness_ids, home, own_cf, Done, Stop};

pub(super) fn run(env: &Env, words: &[String], out: &mut dyn Write) -> Done {
    args::parse(words, &[], Positionals::Refused).map_err(Stop::Said)?;
    // Before anything is made: the launcher would otherwise say its own
    // words for a home it cannot find, and the roster would refuse again.
    home(env)?;
    let prepared = prepare_app(env, &own_cf()?);
    for line in &prepared.report {
        writeln!(out, "{line}")?;
    }
    let found = harness_ids(env);
    let harnesses = if found.is_empty() {
        "none found on PATH".to_owned()
    } else {
        found.join(", ")
    };
    writeln!(out, "harnesses: {harnesses}")?;
    writeln!(
        out,
        "agents: {} saved — manage them with cf ui or cf agent",
        agents_saved(env)?
    )?;
    Ok(())
}
