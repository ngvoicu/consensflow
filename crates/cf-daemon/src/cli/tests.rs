//! The verb's arguments as Node's `parseArgs` reads them (probed on Node
//! v26.8.1, strict, positionals allowed, `json` and `no-open` booleans), and
//! what it prints for the handle line.

use super::*;

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn flags(json: bool, no_open: bool) -> Result<Flags, String> {
    Ok(Flags { json, no_open })
}

#[test]
fn the_two_options_and_positionals_are_read_as_parse_args_reads_them() {
    for (args, expected) in [
        (&[][..], flags(false, false)),
        (&["--json"], flags(true, false)),
        (&["--no-open"], flags(false, true)),
        (&["--no-open", "--json"], flags(true, true)),
        (&["--json", "--json"], flags(true, false)),
        // Positionals are allowed and ignored.
        (&["pos", "--json"], flags(true, false)),
        (&["-"], flags(false, false)),
        // After `--` every word is a positional, an option's look included.
        (&["--no-open", "--", "--foo"], flags(false, true)),
        (&["--", "--json"], flags(false, false)),
        (&["--"], flags(false, false)),
    ] {
        assert_eq!(parse(&words(args)), expected, "{args:?}");
    }
}

#[test]
fn what_it_does_not_know_is_refused_in_the_words_of_node_which_has_no_closing_quote() {
    let unknown = |option: &str| {
        Err(format!(
            "Unknown option '{option}'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- \"{option}\""
        ))
    };
    for (args, expected) in [
        (&["--foo"][..], unknown("--foo")),
        (&["--foo=bar"], unknown("--foo")),
        (&["--jso"], unknown("--jso")),
        (&["--JSON"], unknown("--JSON")),
        (&["---x"], unknown("---x")),
        (&["--=x"], unknown("--=x")),
        (&["-j"], unknown("-j")),
        (&["-jx"], unknown("-j")),
        (&["-ab"], unknown("-a")),
        (&["-j=1"], unknown("-j")),
        (&["--json", "-x"], unknown("-x")),
    ] {
        assert_eq!(parse(&words(args)), expected, "{args:?}");
    }
}

#[test]
fn a_boolean_option_that_is_given_a_value_is_refused_and_the_first_refusal_wins() {
    let refused = |option: &str| Err(format!("Option '--{option}' does not take an argument"));
    assert_eq!(parse(&words(&["--json=x"])), refused("json"));
    assert_eq!(parse(&words(&["--no-open=1"])), refused("no-open"));
    assert_eq!(parse(&words(&["--json=x", "--foo"])), refused("json"));
    assert_eq!(parse(&words(&["--json", "--no-open="])), refused("no-open"));
}

fn handle() -> HandleLine {
    HandleLine {
        url: "http://127.0.0.1:43517/".to_owned(),
        token: "ab".repeat(24),
    }
}

#[test]
fn the_app_gets_the_handle_line_as_json_and_nothing_else() {
    let printed = lines(
        Flags {
            json: true,
            no_open: true,
        },
        &handle(),
    )
    .unwrap();
    assert_eq!(
        printed,
        [format!(
            r#"{{"url":"http://127.0.0.1:43517/","token":"{}"}}"#,
            "ab".repeat(24)
        )]
    );
}

#[test]
fn a_person_gets_the_address_with_the_token_in_its_query_and_how_to_stop() {
    let printed = lines(Flags::default(), &handle()).unwrap();
    assert_eq!(
        printed,
        [
            format!("agents: http://127.0.0.1:43517/?token={}", "ab".repeat(24)),
            "Ctrl-C to stop — nothing keeps running after it.".to_owned(),
        ]
    );
}

/// The thread `ui` hands the daemon to: its own, named, with the stack of
/// [`DAEMON_STACK_BYTES`] and not the main thread's (1 MiB on Windows, where a debug build's
/// launch of a chief's window overflowed it once a launch asked its harness its version first,
/// T-1360).
#[test]
fn the_daemon_runs_on_a_thread_of_its_own_and_not_on_the_main_one() {
    let served =
        |_: &Env, _: &[OsString]| i32::from(std::thread::current().name() == Some(DAEMON_THREAD));
    assert_eq!(ui_with(&Env::default(), &[], served), 1);
}

/// What the stack must hold: more than a test thread has (2 MiB) and than the main thread of
/// Windows has (1 MiB), by a margin, so that a launch that nests deeper is not the next to
/// overflow it.
#[test]
fn the_daemons_stack_is_more_than_the_main_threads_of_windows_and_of_a_test() {
    let stack = std::hint::black_box(DAEMON_STACK_BYTES);
    assert!(stack >= 8 * 1024 * 1024, "{stack} bytes");
    // And a thread of it holds a depth of calls that a thread of 2 MiB cannot: 6 MiB of frames.
    fn deeper(frames: usize) -> usize {
        let mut frame = [0u8; 64 * 1024];
        std::hint::black_box(&mut frame);
        if frames == 0 {
            usize::from(frame[0])
        } else {
            deeper(frames - 1) + usize::from(frame[1])
        }
    }
    assert_eq!(on_a_stack_of_its_own(|| deeper(96)).unwrap(), 0);
}

/// A panic of the daemon is the panic of the thread that waited for it: the process ends as it
/// did when the daemon ran on its main thread.
#[test]
fn a_panic_of_the_daemon_is_carried_to_the_thread_that_waits_for_it() {
    let carried = std::panic::catch_unwind(|| {
        let _ = on_a_stack_of_its_own(|| -> i32 { panic!("the daemon's panic") });
    })
    .unwrap_err();
    assert_eq!(
        carried.downcast_ref::<&str>().copied(),
        Some("the daemon's panic")
    );
}
