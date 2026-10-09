//! The machine: the programs it runs, the time it waits and the bytes it draws,
//! as the system gives them and as a machine that fails to give them.

use std::ffi::OsString;
use std::io;
use std::time::{Duration, Instant};

use super::{credentials, sign, Console, Env, Request, Runner, System, Trial};
use crate::process::{Output, Unstarted};

#[test]
fn a_program_is_run_by_the_system_and_answers_what_it_left() {
    let env = Env::from_process();
    let system = System { env: &env };
    let done = system
        .capture(env!("CARGO"), &[OsString::from("--version")])
        .unwrap();
    assert_eq!(done.code, 0);
    assert!(done.stdout.starts_with("cargo "), "{done:?}");

    let missing = system.capture("cf-release-test-no-such-program", &[]);
    assert!(matches!(missing, Err(Unstarted::NotFound { .. })));
}

#[test]
fn time_passes_when_the_system_is_asked_to_wait() {
    let env = Env::default();
    let started = Instant::now();
    System { env: &env }.wait(Duration::from_millis(30));
    assert!(started.elapsed() >= Duration::from_millis(30));
}

#[cfg(unix)]
#[test]
fn the_bytes_the_system_draws_are_its_own_and_differ_every_time() {
    let env = Env::default();
    let system = System { env: &env };
    let (mut first, mut second) = ([0; 24], [0; 24]);
    system.random(&mut first).unwrap();
    system.random(&mut second).unwrap();
    assert_ne!(first, [0; 24]);
    assert_ne!(first, second);
}

/// A machine that has no bytes to draw, and runs nothing.
struct Dry;

impl Runner for Dry {
    fn capture(&self, program: &str, _: &[OsString]) -> Result<Output, Unstarted> {
        panic!("{program} was run on a machine that could not draw a password");
    }

    fn wait(&self, _: Duration) {}

    fn random(&self, _: &mut [u8]) -> io::Result<()> {
        Err(io::Error::other("no entropy"))
    }
}

#[test]
fn a_machine_that_cannot_draw_a_password_or_a_name_ends_the_run_before_it_makes_or_runs_anything() {
    let trial = Trial::new(super::Fake::new());
    // The password is drawn first, and the folder's name after it.
    for (credentials, told) in [
        (Some(credentials()), "could not draw a password"),
        (None, "mkdtemp"),
    ] {
        let request = Request {
            bundle: trial.bundle.path().to_path_buf(),
            credentials,
            tmp: trial.tmp.path().to_path_buf(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let failure = sign(&Dry, &request, &mut console).unwrap_err().to_string();
        assert!(failure.contains(told), "{failure}");
        assert_eq!(trial.left_behind(), Vec::<String>::new());
    }
}
