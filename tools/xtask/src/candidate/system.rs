//! What the candidate asks of the machine it runs on, behind one seam so that its
//! tests give it a machine of their own: the programs it runs (the build, the
//! smoke, `ditto`, `codesign`, `git`), the table of processes, and whether a
//! process is still running. [`Real`] is the machine; a test's is a script.

use cf_base::env::Env;

use super::Error;
use crate::process::{self, Captured, Invocation};
use crate::updater_smoke::processes::{self, Row};

/// The machine the candidate is built and installed on.
pub trait System {
    /// Runs `invocation` to its end with this program's own input and output,
    /// which is how a build is seen to go: the status it ended with.
    fn run(&mut self, invocation: &Invocation) -> Result<i32, Error>;

    /// Runs `invocation` to its end and keeps what it wrote.
    fn capture(&mut self, invocation: &Invocation) -> Result<Captured, Error>;

    /// The processes that are running, each with the command it was started by.
    fn table(&mut self) -> Result<Vec<Row>, Error>;

    /// Whether the process `pid` is running: signalled, and not waiting to be reaped.
    fn alive(&mut self, pid: u32) -> bool;
}

/// This machine: its programs, found on the environment xtask was started with.
pub struct Real<'a> {
    env: &'a Env,
}

impl<'a> Real<'a> {
    pub fn new(env: &'a Env) -> Self {
        Self { env }
    }
}

impl System for Real<'_> {
    fn run(&mut self, invocation: &Invocation) -> Result<i32, Error> {
        Ok(process::run(invocation, self.env)?)
    }

    fn capture(&mut self, invocation: &Invocation) -> Result<Captured, Error> {
        Ok(process::capture(invocation, self.env)?)
    }

    fn table(&mut self) -> Result<Vec<Row>, Error> {
        Ok(processes::process_table()?)
    }

    fn alive(&mut self, pid: u32) -> bool {
        processes::alive(pid)
    }
}

/// What this machine does is asked of the machine the test is on: harmless
/// programs, and the table of processes it is itself in.
#[cfg(all(test, unix))]
mod tests {
    use std::process::Stdio;

    use super::*;

    #[test]
    fn the_programs_it_runs_end_with_their_status_and_what_they_wrote_is_kept() {
        let env = Env::default();
        let mut system = Real::new(&env);
        assert_eq!(
            system.run(&Invocation::new("/usr/bin/true", ".")).unwrap(),
            0
        );
        assert_eq!(
            system.run(&Invocation::new("/usr/bin/false", ".")).unwrap(),
            1
        );
        let said = system
            .capture(&Invocation::new("/bin/echo", ".").arg("hello"))
            .unwrap();
        assert_eq!((said.code, said.stdout.as_str()), (0, "hello\n"));
        // A program that is not there is an error that says so, not a status.
        let failed = system
            .run(&Invocation::new("/no/such/program", "."))
            .unwrap_err();
        assert!(matches!(failed, Error::Process(_)), "{failed}");
    }

    #[test]
    fn the_table_is_the_systems_own_and_this_process_is_in_it() {
        let env = Env::default();
        let table = Real::new(&env).table().unwrap();
        let own = table
            .iter()
            .find(|row| row.pid == std::process::id())
            .unwrap();
        assert!(!own.command.is_empty());
    }

    #[test]
    fn a_process_is_alive_while_it_runs_and_not_once_it_has_gone() {
        let env = Env::default();
        let mut system = Real::new(&env);
        assert!(system.alive(std::process::id()));
        // A child that was waited for is gone, and is not taken for one that runs.
        let child = process::spawn(
            &Invocation::new("/usr/bin/true", "."),
            &env,
            Stdio::null(),
            false,
        )
        .unwrap();
        let pid = child.id();
        child.wait_with_output().unwrap();
        assert!(!system.alive(pid));
    }
}
