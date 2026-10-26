//! What `--file` reads, and what the reply shows of it, held to the second
//! review of B6 (2026-10-16): the file is named by its bytes, never by a name
//! that reads the same; it is read only if, opened, it is a file of its own,
//! never a link, a pipe, a device or a folder, and no further than the board
//! takes (2 MiB); a `--file` with no name is a command written wrong, words
//! beside it or not; and the reply and the refusals show a control character
//! escaped, so nothing they print redraws the terminal. The commands are run as
//! a window runs them, against an API that writes down what it is asked.

// The tests' own helpers: a failure in one is the test's.
#![allow(clippy::expect_used)]

mod common;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use cf_board::scripted::{reply, scripted, ScriptedApi};
use common::cf;
use serde_json::{json, Value};

/// What `cf args` did as a window runs it against `api`: its exit code, what it
/// printed and what it said on the error output.
fn window<S: AsRef<OsStr>>(api: &ScriptedApi, args: &[S]) -> (Option<i32>, String, String) {
    let ran = cf(
        args,
        &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
        "",
    );
    (
        ran.status.code(),
        String::from_utf8_lossy(&ran.stdout).into_owned(),
        String::from_utf8_lossy(&ran.stderr).into_owned(),
    )
}

/// The bodies the board was posted.
fn bodies(api: &ScriptedApi) -> Vec<String> {
    api.received()
        .into_iter()
        .filter_map(|received| received.json())
        .map(|body| body["body"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// What the board answers a tell to T-3, still queued.
fn told() -> Value {
    json!({
        "message": { "id": 12, "kind": "note", "state": "queued", "sender": "chief",
            "recipient": "zeus", "task": 3, "preview": "…",
            "questions": null, "choices": null, "createdAt": "2026-10-16T09:00:00.000Z" },
        "task": { "number": 3, "state": "queued", "assignee": "zeus", "requester": "chief",
            "title": "Parser", "blockedBy": [], "pool": "worker", "tier": "standard" },
    })
}

/// `cf tell T-3 --file <path>`'s words, `path` as the system takes it.
fn tell_from(path: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["tell", "T-3", "--file"].map(OsString::from).to_vec();
    args.push(path.as_os_str().to_owned());
    args
}

/// A name in `folder` that is no text: `brief-` then a byte (Unix) or a half
/// of a UTF-16 pair (Windows) that reads as U+FFFD, then `.md`.
fn no_text_name(folder: &Path) -> PathBuf {
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(b"brief-\xff.md".to_vec())
    };
    #[cfg(windows)]
    let name = {
        use std::os::windows::ffi::OsStringExt;
        let wide: Vec<u16> = "brief-"
            .encode_utf16()
            .chain([0xd800])
            .chain(".md".encode_utf16())
            .collect();
        OsString::from_wide(&wide)
    };
    folder.join(name)
}

#[test]
fn a_file_is_named_by_its_bytes_never_by_a_name_that_reads_the_same() {
    let folder = tempfile::tempdir().expect("a folder");
    fs::write(
        folder.path().join("brief-\u{fffd}.md"),
        "The other file's words",
    )
    .expect("the file whose name reads the same");
    let own = no_text_name(folder.path());
    // APFS takes no name that is no UTF-8; ext4 and NTFS do.
    let made = fs::write(&own, "The file's own words").is_ok();
    let api = scripted(vec![reply(200, told())]);
    let (code, _, said) = window(&api, &tell_from(&own));
    if made {
        assert_eq!(code, Some(0), "{said}");
        assert_eq!(bodies(&api), ["The file's own words"]);
    } else {
        assert_eq!(code, Some(1), "{said}");
        assert!(said.starts_with("cf: cannot read "), "{said}");
        assert_eq!(bodies(&api), Vec::<String>::new(), "never the other file");
    }
}

#[cfg(unix)]
#[test]
fn a_link_a_device_or_a_folder_is_refused_by_what_the_open_file_is_and_nothing_is_posted() {
    let folder = tempfile::tempdir().expect("a folder");
    let real = folder.path().join("real.md");
    fs::write(&real, "Words the link leads to").expect("the file");
    let link = folder.path().join("link.md");
    std::os::unix::fs::symlink(&real, &link).expect("the link");
    for (path, what) in [
        (link, "a link"),
        (PathBuf::from("/dev/null"), "a device"),
        (folder.path().to_path_buf(), "a folder"),
    ] {
        let api = scripted(vec![reply(200, told())]);
        let (code, printed, said) = window(&api, &tell_from(&path));
        assert_eq!(
            (code, printed.as_str(), said),
            (
                Some(1),
                "",
                format!(
                    "cf: cannot read {}: it is {what}, not a file\n",
                    path.display()
                )
            )
        );
        assert!(api.received().is_empty(), "{what}: nothing is asked");
    }
}

#[cfg(windows)]
#[test]
fn a_device_is_refused_by_what_the_open_file_is_and_nothing_is_posted() {
    let api = scripted(vec![reply(200, told())]);
    let (code, _, said) = window(&api, &tell_from(Path::new("NUL")));
    assert_eq!(
        (code, said.as_str()),
        (Some(1), "cf: cannot read NUL: it is a device, not a file\n")
    );
    assert!(api.received().is_empty());
}

/// `cf args` as a window runs it, stopped when it is not over within `seconds`:
/// its exit code and error output, or none when it had to be stopped.
#[cfg(unix)]
#[allow(clippy::disallowed_methods)] // The test starts cf itself, to stop one that waits.
fn ended_within<S: AsRef<OsStr>>(
    api: &ScriptedApi,
    args: &[S],
    seconds: u64,
) -> Option<(Option<i32>, String)> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let home = tempfile::tempdir().expect("a home");
    let mut command = Command::new(env!("CARGO_BIN_EXE_cf"));
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if ["CONSENSFLOW_", "CF_", "CHISEL_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            command.env_remove(&*name);
        }
    }
    // A `cf` that waits for a pipe lives until it is stopped: the tests run side by side, and it
    // holds only its streams.
    cf_process::hold_only_the_streams(&mut command);
    let mut child = command
        .args(args)
        .env("CONSENSFLOW_HOME", home.path())
        .env("CONSENSFLOW_TOOL_FOLDERS", "")
        .env("CONSENSFLOW_URL", &api.url)
        .env("CONSENSFLOW_TOKEN", "tok")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cf starts");
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("a wait") {
            let mut said = String::new();
            child
                .stderr
                .take()
                .expect("its error output")
                .read_to_string(&mut said)
                .expect("read");
            return Some((status.code(), said));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

#[cfg(unix)]
#[allow(clippy::disallowed_methods)] // `mkfifo` makes the pipe: bare, it runs to its end at once.
#[test]
fn a_pipe_no_one_writes_is_refused_at_once_and_never_waited_for() {
    let folder = tempfile::tempdir().expect("a folder");
    let pipe = folder.path().join("brief.md");
    let made = std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .expect("mkfifo runs");
    assert!(made.success());
    let api = scripted(vec![reply(200, told())]);
    let ended = ended_within(&api, &tell_from(&pipe), 20);
    assert_eq!(
        ended,
        Some((
            Some(1),
            format!(
                "cf: cannot read {}: it is a pipe, not a file\n",
                pipe.display()
            )
        )),
        "cf waited for a writer"
    );
    assert!(api.received().is_empty());
}

#[test]
fn a_file_is_read_as_far_as_the_board_takes_and_one_longer_is_refused_unread() {
    let folder = tempfile::tempdir().expect("a folder");
    let most = 2 * 1024 * 1024;
    let whole = folder.path().join("whole.md");
    fs::write(&whole, "x".repeat(most)).expect("the file");
    let api = scripted(vec![reply(200, told())]);
    let (code, _, said) = window(&api, &tell_from(&whole));
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(
        bodies(&api).concat().len(),
        most,
        "as much as the board takes"
    );

    let longer = folder.path().join("longer.md");
    fs::write(&longer, "x".repeat(most + 1)).expect("the file");
    let api = scripted(vec![reply(200, told())]);
    let (code, _, said) = window(&api, &tell_from(&longer));
    assert_eq!(
        (code, said),
        (
            Some(1),
            format!(
                "cf: cannot read {}: it holds more than 2 MiB, more than the board takes at once\n",
                longer.display()
            )
        )
    );
    assert!(api.received().is_empty());
}

#[test]
fn a_file_flag_with_no_name_is_written_wrong_whatever_stands_beside_it() {
    for args in [
        vec!["tell", "T-3", "Use the v2 grammar", "--file"],
        vec!["tell", "T-3", "--file", ""],
        vec!["task", "add", "--tier", "standard", "--file"],
        vec!["task", "resume", "T-3", "Go on", "--file"],
    ] {
        let api = scripted(vec![reply(200, told())]);
        let (code, printed, said) = window(&api, &args);
        assert_eq!(
            (code, printed.as_str(), said.as_str()),
            (
                Some(2),
                "",
                "cf: --file takes the file to read the words from: --file brief.md\n"
            ),
            "{args:?}"
        );
        assert!(api.received().is_empty(), "nothing is asked: {args:?}");
    }
}

#[test]
fn the_reply_and_a_refusal_show_a_control_character_escaped_never_raw() {
    let api = scripted(vec![reply(200, told())]);
    let words = "Stop\u{1b}[2K\u{1b}[GDONE";
    let (code, printed, _) = window(&api, &["tell", "T-3", words]);
    assert_eq!(code, Some(0));
    assert_eq!(bodies(&api), [words], "the words go as they were given");
    assert!(
        printed.ends_with("\nSent: 15 characters, ending \"Stop\\u{1b}[2K\\u{1b}[GDONE\".\n"),
        "{printed:?}"
    );
    assert!(!printed.contains('\u{1b}'), "{printed:?}");

    let folder = tempfile::tempdir().expect("a folder");
    let missing = folder.path().join("no\u{1b}[2Jwhere.md");
    let api = scripted(Vec::new());
    let (code, _, said) = window(&api, &tell_from(&missing));
    assert_eq!(code, Some(1));
    assert!(said.contains("no\\u{1b}[2Jwhere.md ("), "{said:?}");
    assert!(!said.contains('\u{1b}'), "{said:?}");
}
