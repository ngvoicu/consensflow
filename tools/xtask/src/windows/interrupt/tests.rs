use super::*;

#[test]
fn nothing_is_heard_until_a_stop_comes_and_then_every_copy_has_heard_it() {
    let interrupt = Interrupt::default();
    let copy = interrupt.clone();
    assert!(!interrupt.heard());
    assert!(
        !interrupt.heard_after_failure(),
        "the default waits for none"
    );

    interrupt.raise();
    assert!(copy.heard());
    // What has come is not waited for, however long the wait would be.
    let patient = Interrupt::graced(Duration::from_secs(60));
    patient.raise();
    let started = Instant::now();
    assert!(patient.heard_after_failure());
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn a_stop_that_comes_within_the_grace_is_heard_and_one_that_comes_after_it_is_not() {
    let interrupt = Interrupt::graced(Duration::from_secs(60));
    let late = interrupt.clone();
    let waiting = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        late.raise();
    });
    assert!(interrupt.heard_after_failure());
    waiting.join().unwrap();

    // Not within it: a grace that is over before the stop comes.
    let brief = Interrupt::graced(Duration::from_millis(20));
    assert!(!brief.heard_after_failure());
    assert!(!Interrupt::default().heard_after_failure());
}

/// What a program that listens for what stops it hears when nothing stops it:
/// nothing. It is a program of its own (the test below starts it, as it does the
/// one after it), for listening takes the signals of whoever listens: the Ctrl-C of
/// whoever runs the tests would be heard by no one.
#[test]
#[ignore = "a program the tests of interrupt start"]
fn helper_listens_and_hears_nothing() {
    let interrupt = Interrupt::listen().unwrap();
    // It waits half a second for a stop that a failure may have been the doing of.
    assert_eq!(interrupt.grace, Duration::from_millis(500));
    assert!(!interrupt.heard_after_failure(), "nothing stopped it");
}

/// The test program, started to run `helper`: what it ended with.
fn run_helper(helper: &str) -> crate::process::Captured {
    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let this = std::env::current_exe().unwrap();
    let asked = Invocation::new(this, ".").args(["--ignored", "--exact", helper]);
    process::capture(&asked, &Env::from_process()).unwrap()
}

#[test]
fn a_program_that_listens_hears_nothing_until_a_stop_comes_on_any_system() {
    let ran = run_helper("windows::interrupt::tests::helper_listens_and_hears_nothing");
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
}

/// What a program that listens for what stops it is told by a hangup.
#[cfg(unix)]
#[test]
#[ignore = "a program the tests of interrupt start"]
fn helper_hears_a_hangup() {
    use nix::sys::signal::{raise, Signal};

    let interrupt = Interrupt::listen().unwrap();
    assert!(!interrupt.heard(), "none has come");
    raise(Signal::SIGHUP).unwrap();
    // The listening thread is told when it has its turn: as long as that takes.
    let until = Instant::now() + Duration::from_secs(30);
    while !interrupt.heard() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(interrupt.heard(), "a hangup was not heard");
}

#[cfg(unix)]
#[test]
fn a_program_that_listens_hears_a_hangup_and_is_not_ended_by_it() {
    let ran = run_helper("windows::interrupt::tests::helper_hears_a_hangup");
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
}
