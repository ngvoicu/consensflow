use std::task::Waker;

use super::*;

fn asked(stops: &mut Stops) -> Poll<()> {
    stops.poll(&mut Context::from_waker(Waker::noop()))
}

#[cfg(unix)]
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn no_stop_is_heard_where_none_is_listened_for() {
    assert!(asked(&mut Stops::never()).is_pending());
}

#[cfg(unix)]
#[test]
fn a_stop_is_heard_when_its_signal_comes_and_not_before() {
    use std::future::poll_fn;

    use nix::sys::signal::{raise, Signal};

    let runtime = runtime();
    let mut stops = {
        let _inside = runtime.enter();
        Stops::on(&[SignalKind::user_defined1()]).unwrap()
    };
    assert!(asked(&mut stops).is_pending(), "none has come");

    raise(Signal::SIGUSR1).unwrap();
    // The runtime tells it to who listens when it has its turn: here it waits for the stop.
    runtime.block_on(poll_fn(|cx| stops.poll(cx)));
    assert!(asked(&mut stops).is_pending(), "a stop is heard once");
}

/// What a program that listens for what stops it is told by a hangup. It is a program
/// of its own (the test below starts it), for listening takes the signals of whoever
/// listens: the Ctrl-C of whoever runs the tests would be heard by no one.
#[cfg(unix)]
#[test]
#[ignore = "a program the tests of stops start"]
fn helper_hears_a_hangup() {
    use std::future::poll_fn;

    use nix::sys::signal::{raise, Signal};

    let runtime = runtime();
    let mut stops = {
        let _inside = runtime.enter();
        Stops::listen().unwrap()
    };
    assert!(asked(&mut stops).is_pending(), "none has come");
    raise(Signal::SIGHUP).unwrap();
    runtime.block_on(poll_fn(|cx| stops.poll(cx)));
}

#[cfg(unix)]
#[test]
fn the_stops_a_program_listens_for_include_a_hangup() {
    use cf_base::env::Env;

    use crate::process::{self, Invocation};

    let this = std::env::current_exe().unwrap();
    let asked = Invocation::new(this, ".").args([
        "--ignored",
        "--exact",
        "plants::stops::tests::helper_hears_a_hangup",
    ]);
    let ran = process::capture(&asked, &Env::from_process()).unwrap();
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
}
