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

use cf_e2e::muse::members::{echo_worker, project, the_session};
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

/// A rig whose Muse turns are `turn_ms` long.
fn muse_rig_of(turn_ms: u64) -> Option<Config> {
    muse_rig().map(|config| config.var("CF_TEST_MUSE_DELAY_MS", turn_ms.to_string()))
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

/// The role text of the latest launch of a window of `role` (`chief`, `worker`): the file its hooks read, in the launch's folder
/// (the newest of the rig's, which is the one the chief's window runs on).
fn the_latest_role(rig: &Rig, role: &str) -> Option<String> {
    let launches = rig.home().join("integrations").join("muse");
    let mut roles: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(launches)
        .ok()?
        .flatten()
        .map(|launch| {
            launch
                .path()
                .join(format!("role/.claude/skills/consensflow-{role}/SKILL.md"))
        })
        .filter_map(|file| Some((fs::metadata(&file).ok()?.modified().ok()?, file)))
        .collect();
    roles.sort();
    fs::read_to_string(roles.pop()?.1).ok()
}

/// The latest session that has had its SessionStart hooks run, and its records: what it says of
/// them once they have. (The project's first window, never prompted, has a session of no hook.)
fn hooked_session(rig: &Rig) -> Option<(String, Vec<Value>)> {
    rig.muse_sessions()
        .into_iter()
        .rev()
        .map(|(id, log)| (id, muse::records(&log)))
        .find(|(_, records)| !hook_runs(records).is_empty())
}

/// [`hooked_session`]'s records.
fn hooked(rig: &Rig) -> Option<Vec<Value>> {
    hooked_session(rig).map(|(_, records)| records)
}

/// What ConsensFlow's blocks begin with, a part's frame and a filler alike
/// (`cf_harness::muse::MARK`; this crate holds no harness code, the log is what it reads).
const MARK: &str = "<<consensflow-role";

/// How an unframed role begins: the front matter of its skill file.
const FRONT_MATTER: &str = "---\nname: consensflow-";

/// What a role's blocks look like in a session's log, once checked (`layout`): how many parts
/// the role has, and how many blocks of ConsensFlow's the conversation has (a role that got
/// shorter leaves fillers over the old blocks).
#[derive(Debug, PartialEq, Eq)]
struct Layout {
    parts: usize,
    blocks: usize,
}

/// The id, the part and the number of parts a framed block's head line says.
fn head_of(text: &str) -> Option<(String, usize, usize)> {
    let line = text.lines().next()?.strip_prefix(MARK)?.trim_start();
    let (id, rest) = line.split_once(" part ")?;
    let number: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let of: String = rest
        .split_once(" of ")?
        .1
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    Some((id.to_owned(), number.parse().ok()?, of.parse().ok()?))
}

/// Judges the blocks the SessionStart hooks of a session gave (`blocks`: the last text of
/// each number, as the session's log keeps them) against the role text the launch wrote
/// (`role`): ConsensFlow's own are blocks `0..k` with nothing of the owner's among them; the
/// first `parts` of them, in order, with the frames off, are the role to the byte (an
/// unframed role as Muse cuts it); the rest are fillers of the same role's id; and none of
/// the `old` texts, which an earlier role said, is in any block. What the owner's hooks
/// printed is after ours, and is no part of any of it.
fn layout(role: &str, blocks: &[(u32, String)], old: &[String]) -> Result<Layout, String> {
    let ours = |text: &str| text.starts_with(MARK) || text.starts_with(FRONT_MATTER);
    let mine: Vec<&(u32, String)> = blocks.iter().filter(|(_, text)| ours(text)).collect();
    let numbers: Vec<u32> = mine.iter().map(|(number, _)| *number).collect();
    if numbers != (0..mine.len() as u32).collect::<Vec<_>>() {
        return Err(format!(
            "ConsensFlow's blocks are not the first ones: {numbers:?} of {:?}",
            blocks.iter().map(|(number, _)| *number).collect::<Vec<_>>()
        ));
    }
    let Some((_, first)) = mine.first() else {
        return Err("the conversation has no block of ConsensFlow's".to_owned());
    };
    let parts = match head_of(first) {
        None => {
            if first.as_str() != role.trim() {
                return Err(format!(
                    "the unframed role is not the role text: {} bytes of {}",
                    first.len(),
                    role.len()
                ));
            }
            1
        }
        Some((id, 1, of)) => {
            let mut joined = String::new();
            for (at, (number, text)) in mine.iter().take(of).enumerate() {
                let Some((this, part, parts)) = head_of(text) else {
                    return Err(format!("block {number} is no part of the role"));
                };
                if (this.as_str(), part, parts) != (id.as_str(), at + 1, of) {
                    return Err(format!(
                        "block {number} says {this} part {part} of {parts}, not {id} part {} of {of}",
                        at + 1
                    ));
                }
                if text.trim() != text {
                    return Err(format!("block {number} has white space at its ends"));
                }
                let inner = text
                    .split_once('\n')
                    .and_then(|(_, rest)| rest.rsplit_once('\n'))
                    .ok_or_else(|| format!("block {number} has no frame"))?
                    .0;
                joined += inner;
            }
            if mine.len() < of || joined != role {
                return Err(format!(
                    "the {of} parts are {} bytes, the role {}, and {} blocks are ConsensFlow's",
                    joined.len(),
                    role.len(),
                    mine.len()
                ));
            }
            for (number, text) in &mine[of..] {
                let said = format!("{MARK} {id} part {}: there is no part", number + 1);
                if !text.starts_with(&said) {
                    return Err(format!(
                        "block {number} is no filler of {id}: {}",
                        &text[..text.len().min(90)]
                    ));
                }
            }
            of
        }
        Some((_, part, _)) => return Err(format!("the first block says part {part}")),
    };
    for (number, text) in blocks {
        if let Some(old) = old.iter().find(|old| text.contains(old.as_str())) {
            return Err(format!("block {number} still has old role text: {old}"));
        }
    }
    Ok(Layout {
        parts,
        blocks: mine.len(),
    })
}

/// The human tells the chief `text`: a bracketed paste, and Enter a moment after it, in a write of
/// its own. [`Rig::tell`] writes them together, as a terminal does for a paste, and Muse on Windows
/// reads keys a character at a time, so an Enter that comes with the paste is a line break in it
/// and the words wait in the box unsent (T-954's finding; zeewin2 left this case's words so).
/// The first session's hooks run at its first prompt, so the case sends it.
fn tell_the_chief(rig: &Rig, project: &Project<'_>, text: &str) -> cf_e2e::Result<()> {
    rig.wait_for("the chief to be idle", secs(60), || {
        Ok(project.lane("chief")?["activity"]["state"] == "idle")
    })?;
    let pane = project.lane("chief")?["pane"].clone();
    let send = |bytes: &str| {
        let answer = rig.host(
            "pane.input",
            json!({ "id": pane["id"], "generation": pane["generation"], "bytes": bytes.as_bytes() }),
        )?;
        if answer["ok"] == true {
            Ok(())
        } else {
            Err(cf_e2e::Error::Daemon(format!(
                "pane.input was refused: {answer}"
            )))
        }
    };
    send(&format!("\u{1b}[200~{text}\u{1b}[201~"))?;
    std::thread::sleep(Duration::from_secs(if cfg!(windows) { 8 } else { 1 }));
    send("\r")
}

#[test]
fn a_muse_chief_with_the_largest_role_is_given_all_of_it_by_its_hooks() -> Outcome {
    let Some(config) = muse_rig() else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let project = largest_chiefs_project(&rig)?;
    tell_the_chief(&rig, &project, "Reply with exactly: ROLE_GIVEN")?;
    // The hooks' runs are written as they end and their blocks after the last: the turn that
    // follows them has ended when the log is read whole.
    rig.wait_for(
        "the chief's Muse to run its SessionStart hooks and answer",
        secs(120),
        || Ok(hooked(&rig).is_some_and(|log| !muse::terminals(&log).is_empty())),
    )?;
    let log = hooked(&rig).ok_or("the chief's session has no hook run")?;
    let role = the_latest_role(&rig, "chief").ok_or("the chief's launch wrote no role")?;
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
    // And the context they gave is the whole role, in order, to the byte.
    let blocks = hook_blocks(&log);
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
    let laid = layout(&role, &blocks, &[]).map_err(|why| format!("the blocks: {why}"))?;
    assert!(laid.parts >= 2, "{laid:?}");
    // Muse puts its blocks in a request by their `order`, and gives a hook's block the one of
    // its place: each part is where its number says, ascending, and every byte is in the
    // request's developer context (the one message of the wire's `context` lane).
    let orders = muse::hook_block_orders(&log);
    assert!(
        orders
            .iter()
            .all(|(number, order)| *order == 900_000 + u64::from(*number)),
        "{orders:?}"
    );
    let given: usize = blocks.iter().map(|(_, text)| text.len()).sum();
    let carried = muse::request_context_bytes(&log);
    assert!(
        carried.last().is_some_and(|bytes| *bytes >= given as u64),
        "the request's context is {carried:?} bytes, the blocks {given}"
    );
    rig.close()?;
    Ok(())
}

/// The hooks of the owner's Muse, in the rig's home: SessionStart hooks that each print the file
/// they name as it is (`cat`, `type` on Windows), as a person's would print what a script said.
/// What a hook prints is changed between launches by writing its file ([`owners_say`]).
fn owner_hooks(rig: &Rig, files: &[PathBuf]) -> cf_e2e::Result<()> {
    let config =
        PathBuf::from(rig.var("XDG_CONFIG_HOME").ok_or_else(|| {
            cf_e2e::Error::Daemon("the rig's Muse has no config folder".to_owned())
        })?)
        .join("muse");
    let hooks: Vec<Value> = files
        .iter()
        .map(|file| {
            let file = file.to_string_lossy();
            json!({
                "type": "command",
                "command": format!("cat '{file}'"),
                "commandWindows": format!("type {file}"),
                "timeout": 10,
            })
        })
        .collect();
    cf_e2e::files::write(
        &config.join("settings.json"),
        json!({
            "schema_version": 1,
            "hooks": { "SessionStart": [{ "matcher": "", "hooks": hooks }] },
        })
        .to_string(),
    )
}

/// What each hook of the owner's prints from now on: its text, or nothing.
fn owners_say(files: &[PathBuf], says: &[Option<&str>]) -> cf_e2e::Result<()> {
    for (file, says) in files.iter().zip(says) {
        cf_e2e::files::write(file, says.unwrap_or(""))?;
    }
    Ok(())
}

/// Rules of the most bytes and lines the ledger takes, in characters a hook's answer writes in
/// two bytes, led by `tag`, a word of this version of the rules that no other has: a role that
/// has them is three parts for a chief and two for a member, where a short rule is two and one.
fn heavy_rules(tag: &str) -> String {
    let mut lines = vec![format!("RULESTAG-{tag}")];
    let each = (8_000 - lines[0].len() - 149) / 149;
    lines.extend((0..149).map(|_| "\"\\".repeat(each / 2) + &"\"".repeat(each % 2)));
    let mut rules = lines.join("\n");
    rules += &"\"".repeat(8_000 - rules.len());
    assert_eq!((rules.len(), rules.split('\n').count()), (8_000, 150));
    rules
}

/// Short rules, led by `tag`.
fn short_rules(tag: &str) -> String {
    format!("RULESTAG-{tag}\nBe brief.")
}

/// Says the rules of `project` (none where `None`).
fn set_rules(rig: &Rig, project: &Project<'_>, rules: Option<&str>) -> cf_e2e::Result<()> {
    let said = rig.page(
        "project.rules",
        json!({ "project": project.id(), "rules": rules }),
    )?;
    assert_eq!(said["ok"], true, "{said}");
    Ok(())
}

/// What a launch of the chief left: the conversation it is in, the role text the launch wrote, and
/// the last text of each block of the hooks in the conversation's log.
type Launched = (String, String, Vec<(u32, String)>);

/// Every launch of a chief, as a person quits the app and opens it again: the project is closed
/// and resumed, the chief is told a word, and the case waits for its turn to end (its hooks
/// ran before it). `launch` is the number of this launch of the case (the first is 1).
fn chief_launch(rig: &Rig, project: &Project<'_>, launch: usize) -> cf_e2e::Result<Launched> {
    // The project's own first window opened before the rules were said, and is never
    // prompted: each launch of the case is a window opened after it (the window of the project
    // is the first, so the `launch`th of the case is the `launch + 1`th).
    crate::specs::relaunch(rig, project, launch + 1)?;
    tell_the_chief(rig, project, "Reply with exactly: ROLE_GIVEN")?;
    rig.wait_for(
        &format!("the chief's turn {launch} to end"),
        secs(180),
        || Ok(hooked(rig).is_some_and(|log| muse::terminals(&log).len() >= launch)),
    )?;
    let said = |why: &str| cf_e2e::Error::Daemon(why.to_owned());
    let (session, log) =
        hooked_session(rig).ok_or_else(|| said("the chief's session has no hook run"))?;
    let role =
        the_latest_role(rig, "chief").ok_or_else(|| said("the chief's launch wrote no role"))?;
    Ok((session, role, hook_blocks(&log)))
}

#[test]
fn a_resumed_muse_chief_has_no_old_role_text_whatever_its_owners_hooks_print_and_however_its_role_changes(
) -> Outcome {
    let Some(config) = muse_rig() else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let owner = tempfile::tempdir()?;
    let files = [owner.path().join("one.txt"), owner.path().join("two.txt")];
    owners_say(&files, &[None, None])?;
    owner_hooks(&rig, &files)?;
    rig.stand_ins(&json!({ "schemaVersion": 1, "agents": [
        { "id": "chief", "kind": "muse", "model": "muse-spark-1.3", "effort": "minimal" }
    ]}))?;
    let project = Project::open(&rig, "chief", json!({ "staff": [] }))?;
    // The chief's launches. Each says what the role is (rules of one version, heavy or short),
    // and what each of the owner's two hooks prints this launch: both, none, one and then the
    // other, so that the owner's blocks come and go and the role's size does too (3 parts for
    // heavy rules, 2 for short) and stays the same across a resume.
    let steps: [(Option<String>, [Option<&str>; 2]); 7] = [
        (
            Some(heavy_rules("A")),
            [Some("OWNER-ONE-A"), Some("OWNER-TWO-A")],
        ),
        (Some(heavy_rules("B")), [None, None]), // the same size, the owner silent: the review's case
        (Some(short_rules("C")), [Some("OWNER-ONE-C"), None]), // shorter, the owner prints one
        (Some(heavy_rules("D")), [None, Some("OWNER-TWO-D")]), // longer
        (Some(short_rules("E")), [None, None]), // shorter, the owner silent
        (
            Some(short_rules("F")),
            [Some("OWNER-ONE-F"), Some("OWNER-TWO-F")],
        ), // the same size
        (Some(heavy_rules("G")), [None, Some("OWNER-TWO-G")]),
    ];
    let mut old: Vec<String> = Vec::new();
    let mut covered = 0;
    let mut conversation: Option<String> = None;
    for (at, (rules, says)) in steps.iter().enumerate() {
        let launch = at + 1;
        set_rules(&rig, &project, rules.as_deref())?;
        owners_say(&files, says)?;
        let (session, role, blocks) = chief_launch(&rig, &project, launch)?;
        let laid = layout(&role, &blocks, &old)
            .map_err(|why| format!("launch {launch} ({}): {why}", rules_tag(&role)))?;
        // Parts as the rules make them; blocks never fewer than any launch had.
        let heavy = rules.as_deref().is_some_and(|rules| rules.len() > 1_000);
        assert_eq!(
            laid.parts,
            if heavy { 3 } else { 2 },
            "launch {launch}: {laid:?}"
        );
        covered = covered.max(laid.parts);
        assert_eq!(
            laid.blocks, covered,
            "launch {launch}: a block of ours for every part ever said"
        );
        // What the owner's hooks print this launch is after ours.
        for text in says.iter().flatten() {
            assert!(
                blocks
                    .iter()
                    .any(|(number, block)| block == text && *number as usize >= laid.blocks),
                "launch {launch}: {text} is not among the blocks after ours: {:?}",
                blocks.iter().map(|(number, _)| number).collect::<Vec<_>>()
            );
        }
        old.push(format!("RULESTAG-{}", rules_tag(&role)));
        old.retain(|tag| !role.contains(tag.as_str()));
        // The one conversation went on, from the first launch to the last: each resumed it.
        assert_eq!(
            *conversation.get_or_insert(session.clone()),
            session,
            "launch {launch}: one conversation"
        );
        assert!(
            launch == 1 || muse::members::last_resumed(&rig, &session),
            "launch {launch} did not resume it: {:?}",
            rig.muse_launches().last()
        );
    }
    rig.close()?;
    Ok(())
}

/// The tag of the rules a role says (`RULESTAG-<tag>`), for a message.
fn rules_tag(role: &str) -> String {
    role.split("RULESTAG-")
        .nth(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_resumed_muse_member_goes_from_one_block_to_two_and_back_with_no_old_role_text() -> Outcome {
    let Some(config) = muse_rig_of(500) else {
        return Ok(());
    };
    let rig = Rig::start(config)?;
    let owner = tempfile::tempdir()?;
    let files = [owner.path().join("one.txt")];
    owners_say(&files, &[Some("OWNER-ONE-1")])?;
    owner_hooks(&rig, &files)?;
    let project = project(&rig, &echo_worker())?;
    // A worker's role with no rules is one block, unframed as it always was; with the heaviest
    // rules it is two, framed; and back. Each task opens its worker again on its conversation.
    let steps: [(Option<String>, Option<&str>, usize); 5] = [
        (None, Some("OWNER-ONE-1"), 1),
        (Some(heavy_rules("B")), None, 2),
        (Some(short_rules("C")), Some("OWNER-ONE-C"), 1),
        (Some(heavy_rules("D")), None, 2),
        (None, None, 1),
    ];
    let mut old: Vec<String> = Vec::new();
    let mut covered = 1;
    for (at, (rules, says, parts)) in steps.iter().enumerate() {
        let number = at + 1;
        set_rules(&rig, &project, rules.as_deref())?;
        owners_say(&files, &[*says])?;
        let brief = if number == 1 {
            format!("DISPATCH --tier standard Reply with exactly: T{number}_DONE")
        } else {
            rig.tell(project.id(), &format!("CF task accept T-{}", number - 1))?;
            format!(
                "DISPATCH --tier standard --after T-{} Reply with exactly: T{number}_DONE",
                number - 1
            )
        };
        rig.tell(project.id(), &brief)?;
        rig.wait_for(&format!("T-{number} to be answered"), secs(180), || {
            Ok(project.task(number as i64)?["state"] == "done")
        })?;
        let (session, log) = the_session(&rig);
        assert_eq!(muse::terminals(&log).len(), number, "{session}");
        let role = the_latest_role(&rig, "worker").ok_or("the worker's launch wrote no role")?;
        let blocks = hook_blocks(&log);
        let laid = layout(&role, &blocks, &old)
            .map_err(|why| format!("task {number} ({}): {why}", rules_tag(&role)))?;
        assert_eq!(laid.parts, *parts, "task {number}: {laid:?}");
        covered = covered.max(laid.parts);
        assert_eq!(
            laid.blocks, covered,
            "task {number}: a block of ours for every part ever said"
        );
        old.push(format!("RULESTAG-{}", rules_tag(&role)));
        old.retain(|tag| !role.contains(tag.as_str()) && tag != "RULESTAG-");
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
