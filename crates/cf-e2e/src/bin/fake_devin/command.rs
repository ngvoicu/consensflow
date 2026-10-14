//! A command the agent leaves running when its turn ends, as a harness's shell tool runs
//! one in the background (a line `BACKGROUND <seconds> <words>`): a shell, a child of the
//! window's program, that runs `sleep` for the seconds and waits for it. The agent runs it
//! once it has thought, which is well after the daemon marked the turn, so that what it
//! starts is the turn's work. Its process is written where the rig looks for the jobs a
//! window left (`jobs/job-<window>-1.pid`, beside the daemon's home).
//!
//! A command that reports (a ping says a line a second) goes on saying so on Devin's wire
//! for as long as it runs, once the turn that began it has ended, as a tool says what a
//! call it began does: a thread of its own writes it, and the window's turn is over.

use std::path::Path;
use std::thread;
use std::time::Duration;

use cf_e2e::process::{Run, Spawned};

use crate::said;
use crate::wire::Wire;

/// How long the agent thinks before it runs a tool: well past the daemon's pass (a
/// second), which is how late it may say the turn of a launch has begun.
pub const THINKING: Duration = Duration::from_secs(4);

/// How often the command says a line.
const EVERY: Duration = Duration::from_millis(200);

/// What the command says.
const LINE: &str = "Reply from 127.0.0.1: bytes=32 time<1ms TTL=128";

/// The command the agent began: its shell, which the handle ends the program of if it
/// is dropped, so it is held until the command is over.
pub struct Command {
    shell: Spawned,
}

/// Runs `sleep <seconds>` in a shell, and says on `wire` the call of the tool that runs it
/// (the turn `client` is at work). `window` is the window's own process; the file of its
/// job is in `jobs`.
pub fn start(
    seconds: u64,
    jobs: &Path,
    window: u32,
    wire: &mut Wire,
    client: &str,
) -> Result<Command, String> {
    let command = format!("sleep {seconds}");
    wire.called(&command, client).map_err(said)?;
    // The `true` after it keeps the shell from becoming the command it runs: it stays a
    // shell that waits for it, as a harness's shell tool does.
    let shell = Run::new("/bin/sh")
        .args(["-c", &format!("{command}; true")])
        .inheriting_env()
        .closed_input()
        .spawn()
        .map_err(said)?;
    std::fs::create_dir_all(jobs).map_err(said)?;
    std::fs::write(
        jobs.join(format!("job-{window}-1.pid")),
        format!("{}\n", shell.id()),
    )
    .map_err(said)?;
    Ok(Command { shell })
}

impl Command {
    /// Goes on saying a line on `wire` for as long as the command runs, on a thread of its
    /// own: what a command that reports leaves there once its turn has ended.
    pub fn go_on_reporting(mut self, mut wire: Wire) {
        thread::spawn(move || {
            while !self.shell.has_exited() {
                if wire.reported(LINE).is_err() {
                    break;
                }
                thread::sleep(EVERY);
            }
        });
    }
}
