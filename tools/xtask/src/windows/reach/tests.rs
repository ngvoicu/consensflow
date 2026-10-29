use std::ffi::OsString;

use cf_base::env::Env;

use super::*;

fn context() -> Context {
    Context {
        root: ".".into(),
        env: Env::from_vars(Vec::<(OsString, OsString)>::new()),
    }
}

fn words(invocation: &Invocation) -> Vec<String> {
    invocation
        .args
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_run_is_given_up_135_s_after_the_peers_last_word_and_a_probe_10_s() {
    assert_eq!(RUN.silence_s(), 135);
    assert_eq!(PROBE.silence_s(), 10);
}

#[test]
fn a_blip_of_under_a_minute_is_not_the_end_of_a_run() {
    // The laptop drops for a minute at times, over its tunnel through Wi-Fi: a run's connection
    // outlives twice that, and is still gone within a few minutes of a peer that does not come back.
    assert!(RUN.silence_s() >= 120, "{}", RUN.silence_s());
    assert!(RUN.silence_s() <= 180, "{}", RUN.silence_s());
}

#[test]
fn ssh_has_the_bounds_before_its_host_and_scp_has_them_after_its_quiet_flag() {
    let context = context();
    let ssh = ssh(&context, "zeewin2", RUN);
    assert_eq!(ssh.program, "ssh");
    assert_eq!(
        words(&ssh),
        [
            "-o",
            "ConnectTimeout=30",
            "-o",
            "BatchMode=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=8",
            "zeewin2"
        ]
    );
    let scp = scp(&context, RUN);
    assert_eq!(scp.program, "scp");
    assert_eq!(
        words(&scp),
        [
            "-q",
            "-o",
            "ConnectTimeout=30",
            "-o",
            "BatchMode=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=8"
        ]
    );
}

#[test]
fn a_probe_asks_for_a_short_answer() {
    let probe = ssh(&context(), "zeelin", PROBE);
    assert_eq!(
        words(&probe),
        [
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            "-o",
            "ServerAliveInterval=5",
            "-o",
            "ServerAliveCountMax=1",
            "zeelin"
        ]
    );
}

#[test]
fn what_ssh_says_of_a_peer_it_gave_up_on_is_known_by_its_words() {
    assert!(says_silence("Timeout, server zeewin2 not responding."));
    assert!(says_silence("Timeout, server zeewin2 not responding.\r"));
    // Not what it says of a connection that was refused, closed, or never made.
    for line in [
        "ssh: connect to host 10.6.0.6 port 2222: Operation timed out",
        "Connection to zeewin2 closed by remote host.",
        "Connection timed out during banner exchange",
        "client_loop: send disconnect: Broken pipe",
        "a Timeout, server x not responding.",
        "Timeout, server x is responding",
    ] {
        assert!(!says_silence(line), "{line}");
    }
    assert!(went_silent(
        "noise\nTimeout, server zeewin not responding.\n"
    ));
    assert!(!went_silent("noise\nmore noise\n"));
}

#[test]
fn a_silent_connection_is_told_with_the_machine_and_the_seconds() {
    assert_eq!(
        silent_words("zeewin2", RUN),
        "the connection to zeewin2 went silent for 135 s"
    );
}
