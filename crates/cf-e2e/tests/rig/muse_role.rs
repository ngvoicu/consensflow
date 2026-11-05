//! A Muse chief's role, as the daemon gives it to the machine's own Muse (on Muse's built-in
//! fake model, `--provider echo`: the real program and its hooks, no model, no login, no quota).
//!
//! A Muse window has its role from its SessionStart hook alone, and Muse 1.4.14 takes at most
//! 16,384 bytes of a hook's output: past that the hook `failed` with `output_too_large` and
//! nothing of what it printed was kept (T-1246: every chief of alpha.2, whose text is 17 to 29
//! thousand bytes, started with no role). So the role goes in parts, a hook for each, and the
//! session's log says what each hook did and what context it gave: the real program is the
//! judge here, not a stand-in that knows the limit.
//!
//! Each case is skipped, in plain words, on a machine with no Muse Code installed: CI's.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use cf_e2e::muse::{self, hook_blocks, hook_runs};
use cf_e2e::rig::{Config, Project, Rig};
use serde_json::{json, Value};

use crate::{config, secs, Outcome};

/// How long an echo turn takes: short, nothing here waits on one.
const TURN_MS: u64 = 500;

/// The bytes Muse 1.4.14 takes of a SessionStart hook's output, measured on 2026-11-05
/// (`output_too_large` at 16,385, none at 16,384; plain words and JSON alike).
const MUSE_HOOK_OUTPUT_LIMIT: usize = 16_384;

/// The members of the staff of the largest project: 25, each with two roles.
const STAFF: usize = 25;

/// A rig whose Muse windows run the machine's Muse, or why there is none.
fn muse_rig() -> Option<Config> {
    match muse::installed() {
        Ok(install) => Some(config().muse(
            install.through(env!("CARGO_BIN_EXE_muse-stand-in")),
            TURN_MS,
        )),
        Err(why) => {
            #[allow(clippy::print_stderr)] // Said in the output of the case it skips.
            {
                eprintln!("skipped: {why}");
            }
            None
        }
    }
}

/// The most rules for the staff the ledger takes: 8,000 bytes in 150 lines, the lines in
/// characters of three bytes where they fit (a script outside the Latin ones), so the text
/// that says them is as long as any can make it.
fn the_most_rules() -> String {
    let lines = 150;
    let each = (8_000 - (lines - 1)) / lines;
    let line = format!("{}{}", "日".repeat(each / 3), "a".repeat(each % 3));
    let mut rules = vec![line; lines].join("\n");
    rules += &"a".repeat(8_000 - rules.len());
    assert_eq!((rules.len(), rules.split('\n').count()), (8_000, 150));
    rules
}

/// The project of the largest chief a Muse window is given: a staff of [`STAFF`] who each hold
/// two roles, its specs on, and the most rules for the staff, the chief's window opened after
/// them so that its text has them all. The roster names the chief on Muse and its staff on the
/// stand-in `claude`.
fn largest_chiefs_project(rig: &Rig) -> cf_e2e::Result<Project<'_>> {
    let mut agents = vec![json!({
        "id": "chief", "kind": "muse", "model": "muse-spark-1.3", "effort": "minimal"
    })];
    let mut staff = Vec::new();
    for at in 0..STAFF {
        let name = format!("member-{at:02}");
        agents.push(json!({ "id": name, "kind": "claude-code", "model": "fake" }));
        staff.push(json!({ "agent": name, "roles": ["worker", "reviewer"] }));
    }
    rig.stand_ins(&json!({ "schemaVersion": 1, "agents": agents }))?;
    let project = Project::open(
        rig,
        "chief",
        json!({
            "staff": staff,
            "specs": { "on": true, "folder": "docs/specs", "variant": "tdd" }
        }),
    )?;
    let said = rig.page(
        "project.rules",
        json!({ "project": project.id(), "rules": the_most_rules() }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    // The chief's window opened with the project, before its rules: its next one has them.
    crate::specs::relaunch(rig, &project, 2)?;
    Ok(project)
}

/// The role text of the chief's latest launch: the file its hooks read, in the launch's folder
/// (the newest of the rig's, which is the one the chief's window runs on).
fn the_latest_role(rig: &Rig) -> Option<String> {
    let launches = rig.home().join("integrations").join("muse");
    let mut roles: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(launches)
        .ok()?
        .flatten()
        .map(|launch| {
            launch
                .path()
                .join("role/.claude/skills/consensflow-chief/SKILL.md")
        })
        .filter_map(|file| Some((fs::metadata(&file).ok()?.modified().ok()?, file)))
        .collect();
    roles.sort();
    fs::read_to_string(roles.pop()?.1).ok()
}

/// What the chief's latest session says of its SessionStart hooks: the session's records once
/// the hooks have run in it.
fn hooked(rig: &Rig) -> Option<Vec<Value>> {
    rig.muse_sessions()
        .into_iter()
        .rev()
        .map(|(_, log)| muse::records(&log))
        .find(|records| !hook_runs(records).is_empty())
}

/// The first 40 characters of `text`, for a message.
fn head(text: &str) -> String {
    text.chars().take(40).collect()
}

/// Whether `blocks` are the whole of `role` in order, each as Muse keeps a block: the white
/// space at both ends of what a hook printed cut off. Each block is the next of the role, and
/// between one and the next, and after the last, only line breaks of the role are not in a
/// block: no indentation is lost where a part begins.
fn covers(role: &str, blocks: &[(u32, String)]) -> Result<(), String> {
    let mut rest = role;
    for (at, (number, text)) in blocks.iter().enumerate() {
        let after_break = rest.trim_start_matches('\n');
        let gap = &rest[..rest.len() - after_break.len()];
        if at == 0 && !gap.is_empty() {
            return Err(format!("the role begins with {gap:?}"));
        }
        rest = after_break.strip_prefix(text.as_str()).ok_or_else(|| {
            format!(
                "block {number} ({}…) is not what comes next in the role: {:?}",
                head(text),
                head(after_break)
            )
        })?;
    }
    if rest.chars().all(|character| character == '\n') {
        Ok(())
    } else {
        Err(format!(
            "the role goes on after the last block: {:?}",
            head(rest)
        ))
    }
}

#[test]
fn a_muse_chief_with_the_largest_role_is_given_all_of_it_by_its_hooks() -> Outcome {
    let Some(config) = muse_rig() else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = largest_chiefs_project(&rig)?;
    rig.tell(project.id(), "Reply with exactly: ROLE_GIVEN")?;
    // The hooks' runs are written as they end and their blocks after the last: the turn that
    // follows them has ended when the log is read whole.
    rig.wait_for(
        "the chief's Muse to run its SessionStart hooks and answer",
        secs(120),
        || Ok(hooked(&rig).is_some_and(|log| !muse::terminals(&log).is_empty())),
    )?;
    let log = hooked(&rig).ok_or("the chief's session has no hook run")?;
    let role = the_latest_role(&rig).ok_or("the chief's launch wrote no role")?;
    // The role is the largest ConsensFlow makes: past what one hook says.
    assert!(
        role.len() > MUSE_HOOK_OUTPUT_LIMIT * 3 / 2,
        "the case is the real thing: {} bytes",
        role.len()
    );
    assert!(role.contains("## This project's specs"), "specs on");
    assert!(role.contains("member-24"), "all of the staff");
    // Every hook ended well. (Before T-1246 the one hook ended `failed`: `output_too_large:
    // hook stdout exceeded its 16384-byte ceiling; the process tree was terminated and no
    // stream was parsed`.)
    let runs = hook_runs(&log);
    assert!(
        !runs.is_empty() && runs.iter().all(|(status, _)| status == "completed"),
        "the hooks: {runs:?}"
    );
    // And the context they gave is the whole role, in order.
    let blocks = hook_blocks(&log);
    assert_eq!(
        blocks.iter().map(|(number, _)| *number).collect::<Vec<_>>(),
        (0..blocks.len() as u32).collect::<Vec<_>>(),
        "one block for each hook, from the first"
    );
    assert!(
        blocks
            .iter()
            .all(|(_, text)| text.len() <= MUSE_HOOK_OUTPUT_LIMIT),
        "{:?}",
        blocks
            .iter()
            .map(|(_, text)| text.len())
            .collect::<Vec<_>>()
    );
    if let Err(why) = covers(&role, &blocks) {
        panic!("the blocks are not the role text: {why}");
    }
    rig.close()?;
    Ok(())
}

// What the machine's Muse does with a SessionStart hook's output, measured on it as the cases
// of this file's top rest on: the hooks here print a file's bytes as they are (`cat`, and
// `type` in cmd.exe on Windows), so a case says exactly what a hook printed. Muse Code
// 1.4.14-R7309.1, 2026-11-05 (T-1246); a Muse that changes any of it fails one of these.

/// What one run of the machine's Muse on its echo model did with the hooks `printing` gave it
/// (the bytes each prints), in a home of its own: its session's records.
fn muse_run(printing: &[&[u8]], session: Option<&str>) -> Found<Vec<Value>> {
    muse_run_in(&tempfile::tempdir()?, printing, session)
}

/// [`muse_run`] in the home `home`, whose sessions earlier runs left: a run on the session
/// they began (`session`) is a start of it again, as a resume is. The records are those of the
/// session the last run wrote.
fn muse_run_in(
    home: &tempfile::TempDir,
    printing: &[&[u8]],
    session: Option<&str>,
) -> Found<Vec<Value>> {
    let install = muse::installed()?;
    let at = |below: &str| home.path().join(below);
    let mut hooks = Vec::new();
    for (number, bytes) in printing.iter().enumerate() {
        let file = at(&format!("hook-{number}.txt"));
        fs::write(&file, bytes)?;
        let file = file.to_string_lossy().into_owned();
        hooks.push(json!({
            "type": "command",
            "command": format!("cat '{file}'"),
            "commandWindows": format!("type {file}"),
            "timeout": 10,
        }));
    }
    let settings = json!({ "SessionStart": [{ "matcher": "", "hooks": hooks }] });
    let workspace = at("work");
    fs::create_dir_all(&workspace)?;
    let mut run = cf_e2e::process::Run::new(&install.binary)
        .args([
            "exec",
            "--provider",
            "echo",
            "--json",
            "--contributor-mode",
            "off",
        ])
        .arg("--workspace")
        .arg(&workspace)
        .arg("-c")
        .arg(format!("hooks={settings}"));
    if let Some(session) = session {
        run = run.arg("--session-id").arg(session);
    }
    // Muse's hooks run in a shell of their own: cmd.exe on Windows, which a program is given
    // the means to find there, and the system's `cat` on a Mac.
    let ran = run
        .arg("x")
        .vars([
            ("HOME", home.path().as_os_str()),
            ("USERPROFILE", home.path().as_os_str()),
            ("APPDATA", at("appdata").as_os_str()),
            ("LOCALAPPDATA", at("localappdata").as_os_str()),
            ("XDG_CONFIG_HOME", at(".config").as_os_str()),
            ("XDG_DATA_HOME", at(".local/share").as_os_str()),
            ("XDG_CACHE_HOME", at(".cache").as_os_str()),
            ("MUSE_NO_AUTO_UPDATE", "1".as_ref()),
        ])
        .var("PATH", "/usr/bin:/bin")
        .finding_programs()
        .closed_input()
        .limit(Duration::from_secs(120))
        .run()?;
    assert_eq!(ran.code, Some(0), "muse exec: {ran}");
    let sessions = muse::sessions(home.path());
    let (_, log) = sessions
        .last()
        .ok_or("the run left no session in its home")?;
    Ok(muse::records(log))
}

/// What a helper of these cases finds, or why it cannot.
type Found<T> = Result<T, Box<dyn std::error::Error>>;

/// What a hook says over a block it wrote before.
const OVERWRITE: &str = "(nothing)";

/// `n` bytes of `a`.
fn bytes_of(n: usize) -> Vec<u8> {
    vec![b'a'; n]
}

/// Claude's JSON answer of exactly `n` bytes in all, its text `a`s.
fn json_of(n: usize) -> Vec<u8> {
    let head = br#"{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":""#;
    let tail = br#""}}"#;
    let mut answer = head.to_vec();
    answer.extend(bytes_of(n - head.len() - tail.len()));
    answer.extend(tail);
    answer
}

#[test]
fn muse_takes_16384_bytes_of_a_hooks_output_and_no_more_plain_or_in_json() {
    if muse_rig().is_none() {
        return;
    }
    for (name, printed) in [
        ("plain", bytes_of as fn(usize) -> Vec<u8>),
        ("json", json_of),
    ] {
        let whole = muse_run(&[&printed(MUSE_HOOK_OUTPUT_LIMIT)], None).unwrap();
        let runs = hook_runs(&whole);
        assert_eq!(runs, [("completed".to_owned(), None)], "{name}: {runs:?}");
        assert_eq!(hook_blocks(&whole).len(), 1, "{name}: the block is given");
        let over = muse_run(&[&printed(MUSE_HOOK_OUTPUT_LIMIT + 1)], None).unwrap();
        let runs = hook_runs(&over);
        assert_eq!(runs.len(), 1, "{name}: {runs:?}");
        assert_eq!(runs[0].0, "failed", "{name}");
        assert!(
            runs[0]
                .1
                .as_deref()
                .is_some_and(|said| said.starts_with("output_too_large:")
                    && said.contains("16384-byte ceiling")),
            "{name}: {runs:?}"
        );
        assert!(
            hook_blocks(&over).is_empty(),
            "{name}: nothing of it is kept"
        );
    }
}

#[test]
fn muse_keeps_each_hooks_text_as_a_block_of_its_own_numbered_in_order_with_its_white_space_cut_off_and_every_block_is_in_the_request(
) {
    if muse_rig().is_none() {
        return;
    }
    // Three hooks of 16,000 bytes: each a block, in the order of the hooks, whole.
    let (first, second, third) = (
        [b"ONE ".as_slice(), &bytes_of(15_996)].concat(),
        [b"TWO ".as_slice(), &bytes_of(15_996)].concat(),
        [b"THREE ".as_slice(), &bytes_of(15_994)].concat(),
    );
    let log = muse_run(&[&first, &second, &third], None).unwrap();
    assert!(hook_runs(&log)
        .iter()
        .all(|(status, _)| status == "completed"));
    let blocks = hook_blocks(&log);
    assert_eq!(
        blocks,
        [
            (0, String::from_utf8(first).unwrap()),
            (1, String::from_utf8(second).unwrap()),
            (2, String::from_utf8(third).unwrap()),
        ]
    );
    // And each reached the model's request: the developer context of it is at least the
    // 48,000 bytes of the three, and a run with none has the rest of Muse's own.
    let with = muse::request_context_bytes(&log);
    let without = muse::request_context_bytes(&muse_run(&[], None).unwrap());
    assert_eq!((with.len(), without.len()), (1, 1), "{with:?} {without:?}");
    assert!(
        with[0] >= without[0] + 47_900,
        "the request's context is {} bytes with the blocks and {} without",
        with[0],
        without[0]
    );
    // White space at both ends of a hook's text is cut off, a blank text gives no block.
    let log = muse_run(
        &[b"\n\n  lead\n  two\n\t tab \n\n", b"  \n\n ", b"next"],
        None,
    )
    .unwrap();
    assert_eq!(
        hook_blocks(&log),
        [
            (0, "lead\n  two\n\t tab".to_owned()),
            (1, "next".to_owned())
        ],
        "a hook that printed only blanks gave no block, and the numbers are of the blocks"
    );
}

#[test]
fn a_resumed_conversation_keeps_the_blocks_it_has_until_a_hook_of_the_same_number_writes_them() {
    if muse_rig().is_none() {
        return;
    }
    // A session whose first start gave three blocks, and whose second, with one hook, writes
    // the first only: the other two are in its request still, the old role's tail.
    let home = tempfile::tempdir().unwrap();
    let session = "11111111-2222-7333-8444-555555555555";
    let first = muse_run_in(&home, &[b"AAA", b"BBB", b"CCC"], Some(session)).unwrap();
    assert_eq!(hook_blocks(&first).len(), 3);
    let kept = muse_run_in(&home, &[b"AAA again"], Some(session)).unwrap();
    assert_eq!(
        hook_blocks(&kept),
        [
            (0, "AAA again".to_owned()),
            (1, "BBB".to_owned()),
            (2, "CCC".to_owned())
        ],
        "the blocks the first start gave, one of them written again"
    );
    let requests = muse::request_context_bytes(&kept);
    let alone = muse::request_context_bytes(&muse_run(&[b"AAA again"], None).unwrap());
    assert!(
        requests.last() > alone.last(),
        "the request of the second start still holds the blocks of the first: {requests:?} for {alone:?}"
    );
    // The launch's hooks over them (T-1246) write what a hook says over the old text.
    let nothing = OVERWRITE.as_bytes();
    let written = muse_run_in(&home, &[b"AAA third", nothing, nothing], Some(session)).unwrap();
    assert_eq!(
        hook_blocks(&written),
        [
            (0, "AAA third".to_owned()),
            (1, OVERWRITE.to_owned()),
            (2, OVERWRITE.to_owned())
        ],
        "no text of the old role is left in the conversation"
    );
}
