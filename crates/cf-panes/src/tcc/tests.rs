//! The reading of tccd's log (T-1194) on lines recorded from a real Mac
//! (`tests/fixtures/tcc`), and on those lines made otherwise only where a test says how.

use super::lines::{read_line, Joiner, Line, Prompt};

const MEDIA_LIBRARY: &str = include_str!("../../tests/fixtures/tcc/media-library-prompt.ndjson");
const OWN_CLIENT: &str = include_str!("../../tests/fixtures/tcc/own-client-prompt.ndjson");
const BACKGROUND: &str = include_str!("../../tests/fixtures/tcc/background-queries.ndjson");
const APP_PREFLIGHT: &str = include_str!("../../tests/fixtures/tcc/app-preflight.ndjson");
const BANNER: &str = include_str!("../../tests/fixtures/tcc/stream-banner.txt");

/// The pid of the stand-in app whose window the recorded prompt came from, and of its `ls`.
const APP: u32 = 22911;
const LS: u32 = 22913;

/// What `joiner` makes of every line of `recorded`, in order.
fn fed(joiner: &mut Joiner, recorded: &str) -> Vec<Prompt> {
    recorded
        .lines()
        .filter_map(|line| match read_line(line) {
            Line::Event { tccd, message } => joiner.feed(tccd, &message),
            Line::Text(_) => None,
        })
        .collect()
}

#[test]
fn a_recorded_apple_music_dialog_is_one_prompt_naming_the_service_the_process_and_its_program() {
    let prompts = fed(&mut Joiner::new(APP), MEDIA_LIBRARY);

    assert_eq!(
        prompts,
        [Prompt {
            service: "MediaLibrary".to_owned(),
            pid: LS,
            program: "ls".to_owned(),
        }]
    );
}

#[test]
fn a_dialog_raised_in_another_apps_name_is_not_told() {
    assert_eq!(fed(&mut Joiner::new(1), MEDIA_LIBRARY), []);
    assert_eq!(fed(&mut Joiner::new(LS), MEDIA_LIBRARY), []);
}

#[test]
fn a_dialog_the_app_raises_for_itself_is_not_a_windows() {
    // The attribution of an access the app's own process makes has no responsible process:
    // the process is its own.
    assert_eq!(fed(&mut Joiner::new(57904), OWN_CLIENT), []);
}

#[test]
fn what_the_system_only_asks_is_no_dialog() {
    // The queries sandboxd makes where no dialog can be drawn, and a check an app makes of its
    // own grant: requests, with no word of a prompt.
    assert_eq!(fed(&mut Joiner::new(92979), BACKGROUND), []);
    // The recorded check was made in the name of the live app, pid 1811: the joiner is for that
    // app, so that only the lack of a word of a dialog keeps it from being told.
    assert_eq!(fed(&mut Joiner::new(1811), APP_PREFLIGHT), []);
    let responsible = APP_PREFLIGHT
        .lines()
        .filter_map(|line| match read_line(line) {
            Line::Event { message, .. } => Some(message),
            Line::Text(_) => None,
        })
        .find(|message| message.contains("responsible={TCCDProcess: identifier=dev.ngvoicu"))
        .expect("the recorded attribution");
    assert!(responsible.contains("ConsensFlow.app"), "{responsible}");
}

#[test]
fn the_banner_and_a_line_cut_short_are_text_and_an_event_is_an_event() {
    assert!(
        matches!(read_line(BANNER.trim_end()), Line::Text(text) if text.starts_with("Filtering the log data"))
    );
    let event = MEDIA_LIBRARY.lines().next().expect("a line");
    assert!(matches!(
        read_line(event),
        Line::Event { tccd: 636, message } if message.starts_with("AUTHREQ_CTX: msgID=408.2711")
    ));
    assert!(matches!(
        read_line(&event[..event.len() / 2]),
        Line::Text(_)
    ));
    assert!(matches!(read_line(""), Line::Text(_)));
    // JSON that is no event of a process says nothing.
    assert!(matches!(read_line("{\"a\":1}"), Line::Text(_)));
}

#[test]
fn the_lines_of_a_request_join_in_any_order_and_a_request_is_told_once() {
    let events: Vec<(u64, String)> = MEDIA_LIBRARY
        .lines()
        .filter_map(|line| match read_line(line) {
            Line::Event { tccd, message } => Some((tccd, message)),
            Line::Text(_) => None,
        })
        .collect();
    assert_eq!(events.len(), 3);
    // The dialog before the attribution that names who asked.
    let mut reversed = Joiner::new(APP);
    let told: Vec<_> = events
        .iter()
        .rev()
        .filter_map(|(tccd, message)| reversed.feed(*tccd, message))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].pid, LS);
    // The same lines again are the same request.
    let mut twice = Joiner::new(APP);
    let told: Vec<_> = events
        .iter()
        .chain(events.iter())
        .filter_map(|(tccd, message)| twice.feed(*tccd, message))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
}

#[test]
fn two_daemons_message_numbers_do_not_mix() {
    // The system's tccd and the user's both count messages from a sender's number: the same
    // msgID of another tccd is another request.
    let mut joiner = Joiner::new(APP);
    let events: Vec<(u64, String)> = MEDIA_LIBRARY
        .lines()
        .filter_map(|line| match read_line(line) {
            Line::Event { tccd, message } => Some((tccd, message)),
            Line::Text(_) => None,
        })
        .collect();
    // The attribution from one tccd, the dialog from the other.
    assert_eq!(joiner.feed(414, &events[1].1), None);
    assert_eq!(joiner.feed(636, &events[2].1), None);
    // The right pair completes it.
    assert!(joiner.feed(636, &events[1].1).is_some());
}

#[test]
fn a_path_with_spaces_and_commas_is_read_to_its_end() {
    let attribution = "AUTHREQ_ATTRIBUTION: msgID=408.9, attribution={responsible={TCCDProcess: \
        identifier=dev.ngvoicu.consensflow.candidate, pid=500, auid=501, euid=501, \
        responsible_path=/Users/x/Applications/ConsensFlow Candidate.app/Contents/MacOS/app, \
        binary_path=/Users/x/Applications/ConsensFlow Candidate.app/Contents/MacOS/app}, \
        accessing={TCCDProcess: identifier=find, pid=501, auid=501, euid=501, \
        binary_path=/Users/x/My Tools, v2/find}, \
        requesting={TCCDProcess: identifier=com.apple.sandboxd, pid=408, auid=0, euid=0, \
        binary_path=/usr/libexec/sandboxd}, },";
    let prompting =
        "AUTHREQ_PROMPTING: msgID=408.9, service=kTCCServiceSystemPolicyDocumentsFolder, \
        subject=Sub:{x}Resp:{y}, ";
    let mut joiner = Joiner::new(500);
    assert_eq!(joiner.feed(636, attribution), None);
    assert_eq!(
        joiner.feed(636, prompting),
        Some(Prompt {
            service: "SystemPolicyDocumentsFolder".to_owned(),
            pid: 501,
            program: "find".to_owned(),
        })
    );
}

#[test]
fn requests_that_never_become_a_dialog_are_not_kept_for_ever() {
    let mut joiner = Joiner::new(APP);
    for number in 0..5_000 {
        let message = format!(
            "AUTHREQ_ATTRIBUTION: msgID=408.{number}, attribution={{responsible={{TCCDProcess: \
             identifier=a, pid={APP}, binary_path=/a}}, accessing={{TCCDProcess: identifier=ls, \
             pid=7, binary_path=/bin/ls}}, }},"
        );
        assert_eq!(joiner.feed(636, &message), None);
    }
    assert!(joiner.kept() <= 512, "{} kept", joiner.kept());
    // The oldest is forgotten, and a dialog for it names nobody.
    assert_eq!(
        joiner.feed(
            636,
            "AUTHREQ_PROMPTING: msgID=408.0, service=kTCCServiceMediaLibrary, subject=Sub:{a}Resp:{b}, "
        ),
        None
    );
}
