//! `cf doctor` (`doctor`, `bin/cf.mjs`): what ConsensFlow sees on this
//! machine, a line each: the version, the home, the harnesses, the agents, the
//! roles, what the terminal command runs, and the hooks an older version left
//! in Claude Code's settings. It reads and writes nothing of its own, and the
//! words after it are no matter, but the one that asks for its usage (`--help`,
//! `-h`), which `run` answers before it gets here.
//!
//! A line is said as soon as it is known, so what stops it later (a file of
//! agents that cannot be read, or a command that cannot) is said after the
//! lines before it, as Node said it.

use std::io::Write;

use cf_base::env::Env;
use cf_harness::claude::stale_hooks;
use cf_launcher::{runtime, Places};

use super::{agents_saved, harness_ids, home, own_cf, Done, Stop};

pub(super) fn run(env: &Env, out: &mut dyn Write) -> Done {
    // Before a line is said: there is no home to name.
    let home = home(env)?;
    let cf = own_cf()?;
    let found = harness_ids(env);
    writeln!(out, "consensflow {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "home:         {}", home.to_string_lossy())?;
    let harnesses = if found.is_empty() {
        "none on PATH".to_owned()
    } else {
        found.join(", ")
    };
    writeln!(out, "harnesses:    {harnesses}")?;
    writeln!(out, "agents:       {}", agents_saved(env)?)?;
    writeln!(
        out,
        "roles:        bundled chief, worker, reviewer and advisor; prepared when a window launches"
    )?;

    // The install records what runs the command. If that has moved, the wiring
    // it left behind stops working, and saying so here is cheaper than letting
    // it fail quietly.
    if let Some(wiring) = runtime(env, &cf, &Places::default()).map_err(Stop::Said)? {
        writeln!(out, "{}", wiring.report())?;
    }

    // Claude Code's settings are not ours to write, so a hook an older version
    // left there is named rather than removed behind the user's back.
    if let Some(line) = stale_hooks(env).report() {
        writeln!(out, "{line}")?;
    }
    Ok(())
}
