//! Live: does each harness's own per-launch knob make it compact where it is told to, on
//! the run's own home? It is opt-in (`cargo xtask live compaction-routes -- <harness>`): it
//! spends the machine's quota on short headless turns of each harness's cheap model, and
//! needs the harness installed and logged in, so it is ignored unless asked for and is no
//! part of `check` or of CI.
//!
//! The routes a member window is given its project's compaction point by (T-1265, SCX-28),
//! each per launch and never in the owner's config:
//!
//! - **Codex** `-c model_auto_compact_token_limit=<tokens>`. Codex compacts a thread past it
//!   at the start of its next turn. Its source caps it at 90% of the model's window
//!   (`ModelInfo::auto_compact_token_limit`, codex-rs 0.166.0), so a size above that leaves
//!   Codex's own point.
//! - **Devin** `DEVIN_COMPACTION_THRESHOLDS=<spawn>,<hard>` (the hidden flag
//!   `--compaction-thresholds`, whose environment variable it is). Devin's binary says
//!   "Ignoring invalid compaction thresholds for model … max_tokens=" for thresholds a model
//!   cannot take.
//! - **Muse** `-c context_compaction.soft_threshold=<fraction>` (beside it `hard_threshold`,
//!   0.75 and 0.9 by default, and `provider_context_limit_tokens`, which replaces the model
//!   catalog's context limit the fractions are of).
//!
//! - **Pi** ConsensFlow's own extension (`CF_COMPACTION_TOKENS`): it asks Pi to compact at a
//!   rest, past the size, and keeps the word of it (`settled/<launch>.compacting.json`) until
//!   the compaction ends, so that the engine does not close the done task's window under the
//!   summary (T-1475: the close, ten seconds after the result, aborted every one). The case
//!   runs a Pi member of a project of the least size, 250,000, through a task of two dozen
//!   reads of 12,000 tokens each, and asks that the member's window is open when Pi's own
//!   session writes a `compaction` entry, which no run has seen yet.
//!
//! Each case runs the harness on a home of the run's own (the kit's [`Isolation`] for Codex
//! and Devin; for Muse a copy of `auth.json` and the model catalog, the login being the
//! keychain's, as `live muse-reads` does) with the knob set low, so that a short turn crosses
//! it, and reads the harness's own record for a compaction:
//!
//! - `codex`: a turn that reads a file, then `exec resume` of the same thread; its rollout
//!   must say `compacted`.
//! - `devin`: one turn of six sequential shell steps at `CF_DEVIN_THRESHOLDS` (`8000,8000`
//!   unless the run says otherwise), whose store must hold a summary (`summarized_from`); it
//!   says the `<context_usage>` notes the model was given.
//! - `muse`: one turn at a soft threshold of 0.03; one at a provider limit of 40,000 tokens;
//!   one at 900,000, above the model's own limit; the session logs say the thresholds in
//!   tokens and what triggered each compaction (`CF_ONLY=<words>` runs one of them).
//!
//! ```text
//! cargo xtask live compaction-routes -- codex
//! cargo xtask live compaction-routes -- devin
//! cargo xtask live compaction-routes -- muse
//! cargo xtask live compaction-routes -- pi
//! ```
//!
//! **Measured (T-1265, 2026-11-06, the Mac).** Codex 0.166.0 (gpt-5.6-luna) at 2000: 2
//! `compacted` records, on zeewin2 too. Muse 1.4.14 (muse-spark-1.3-contributor, minimal): soft
//! 0.03 → soft 15,360 / hard 460,800 (so its real window is 512,000), compactions triggered at
//! 22,505 and 39,289; a limit of 40,000 → 30,000 / 36,000, soft and blocking compactions. Devin
//! 3000.11.234: on swe-2 a two-step turn at `1000,1000` showed the limit in its note (`of 5k`)
//! and wrote no summary (twice); on gpt-6-luna-none a turn of one tool step at `8000,8000`
//! showed `16k of 8k` and wrote none; on swe-2 the six-step turn at `8000,8000` compacted four
//! times (notes 20k, 19k, 14k, 19k, 14k of 8k). Devin's summary comes in the background and is
//! put in at a later step, so a turn needs steps after the size is passed.
//!
//! [`Isolation`]: cf_e2e::live::isolation::Isolation

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::guard::{copy_muse_into, login_in_keychain, Scratch};
use cf_e2e::live::members::{self, find, is_over, Members, Plan};
use cf_e2e::live::{agents, ask, codex, machine, Harness, Log};
use cf_e2e::process::{own_var, Run};
use cf_e2e::rig::HarnessHome;
use cf_e2e::{files, rollout, Error, Result};
use serde_json::{json, Value};

/// The stand-in for the Pi member's chief.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long one headless turn may take.
const TURN: Duration = Duration::from_secs(300);

/// What a turn is asked: read a file of the case's own whose words alone cross the knob.
const ASK: &str = "Run `cat notes.txt` with your shell tool, then run `wc -l notes.txt`, then answer with the single word done.";

/// What the resumed Codex turn is asked: words the resume's turn is found by in the rollout.
const AGAIN: &str = "Answer with the single word again.";

/// What a Devin turn is asked: steps enough to pass the thresholds, each after the one before.
const LONG_ASK: &str = "Run these shell commands one at a time, each only after the one before has finished, never two in one call: `cat notes.txt`, `wc -l notes.txt`, `cat notes.txt`, `wc -w notes.txt`, `cat notes.txt`, `wc -c notes.txt`. Then answer with the single word done.";

/// A file whose words are some 12,000 tokens.
fn notes(folder: &Path) -> Result {
    let line = "These are the notes of the compaction check, a line of filler words to read.\n";
    cf_e2e::door::write(folder.join("notes.txt"), line.repeat(600))
}

/// A folder of the case's own, by the path it resolves to (Codex and Devin know a folder so).
fn scratch_folder() -> Result<(cf_e2e::door::Made, PathBuf)> {
    let scratch = cf_e2e::door::made("live-compaction-routes-").map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let folder = std::fs::canonicalize(scratch.path()).map_err(|source| Error::File {
        action: "resolve",
        path: scratch.path().to_path_buf(),
        source,
    })?;
    notes(&folder)?;
    Ok((scratch, folder))
}

/// The machine's variables a harness started here needs besides its home: where programs are.
fn machine_vars() -> Vec<(String, String)> {
    ["PATH", "PATHEXT", "HOME", "USER", "LANG", "TMPDIR", "SHELL"]
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), own_var(name)?)))
        .collect()
}

/// Every file below `root` whose bytes hold `words`.
fn files_saying(root: &Path, words: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                folders.push(path);
            } else if std::fs::read(&path)
                .map(|bytes| String::from_utf8_lossy(&bytes).contains(words))
                .unwrap_or(false)
            {
                found.push(path);
            }
        }
    }
    found
}

#[test]
#[ignore = "opt-in: spends the machine's quota on two short Codex turns; cargo xtask live compaction-routes -- codex"]
fn codex_compacts_a_thread_past_the_limit_its_launch_gives() -> Result {
    let log = Log::begin("live compaction-routes codex")?;
    ask::prepare()?;
    let install = cf_e2e::live::codex()?;
    let model = agents::model(Harness::Codex);
    let home = install.home_outside_a_rig()?;
    let codex_home = home
        .iter()
        .find(|(name, _)| name == "CODEX_HOME")
        .map(|(_, value)| PathBuf::from(value))
        .ok_or_else(|| Error::Daemon("the kit gave Codex no CODEX_HOME".to_owned()))?;
    let (_scratch, folder) = scratch_folder()?;
    let mut words = vec![
        "-c".to_owned(),
        "model_auto_compact_token_limit=2000".to_owned(),
        "-c".to_owned(),
        format!("model={}", Value::String(model.name.clone())),
        "--skip-git-repo-check".to_owned(),
        "--dangerously-bypass-approvals-and-sandbox".to_owned(),
    ];
    if let Some(effort) = &model.effort {
        words.extend([
            "-c".to_owned(),
            format!("model_reasoning_effort={}", Value::String(effort.clone())),
        ]);
    }
    // The list itself is read on the run's own home, before switching its servers off.
    words.extend(codex::isolation(&codex::mcp_servers(
        install.command(),
        &home,
    )?)?);
    let codex_program = install.command().to_string_lossy().into_owned();
    let run = |args: Vec<String>| {
        let argv = ask::program(&args, &machine_vars())?;
        let (program, rest) = argv
            .split_first()
            .ok_or_else(|| Error::Daemon("no command line".to_owned()))?;
        Run::new(program)
            .args(rest)
            .cwd(&folder)
            .inheriting_env()
            .vars(home.clone())
            .closed_input()
            .limit(TURN)
            .run()
    };
    let mut first = vec![codex_program.clone(), "exec".to_owned()];
    first.extend(words.clone());
    first.push(ASK.to_owned());
    let ran = run(first)?;
    log.say(format!("codex exec: exit {:?}", ran.code));
    let first_code = ran.code;
    let mut second = vec![
        codex_program.clone(),
        "exec".to_owned(),
        "resume".to_owned(),
        "--last".to_owned(),
    ];
    second.extend(words);
    second.push(AGAIN.to_owned());
    let ran = run(second)?;
    log.say(format!("codex exec resume: exit {:?}", ran.code));
    let second_code = ran.code;
    let rollouts = rollout::rollouts(&codex_home);
    let mut compacted = 0;
    // The resume's own turn: whether the thread was compacted between its start and its
    // message, as Codex compacts a thread past the limit at the start of its next turn.
    let mut resumed = None;
    for path in &rollouts {
        let records = rollout::records(&rollout::read(path)?);
        compacted += records
            .iter()
            .filter(|record| record["type"] == "compacted")
            .count();
        let turn = rollout::turn_taking(&records, AGAIN);
        if turn.taken {
            resumed = Some(turn);
        }
    }
    log.block(
        &format!("codex {} on {}", install.version(), model.name),
        &format!(
            "{} rollout(s); `compacted` records: {compacted}; the resume's turn: {resumed:?}",
            rollouts.len()
        ),
    );
    install.finish_homes()?;
    assert_eq!(first_code, Some(0), "codex exec failed");
    assert_eq!(second_code, Some(0), "codex exec resume failed");
    assert!(compacted > 0, "Codex never compacted at a limit of 2000");
    let resumed = resumed.expect("the resume's message is in the thread's rollout");
    assert!(
        resumed.compacted.is_some(),
        "the resumed turn was not compacted at its start: {resumed:?}"
    );
    log.passed();
    Ok(())
}

/// Copies every file below `from` into `to`, but those whose name holds a word of `left`.
fn copy_tree(from: &Path, to: &Path, left: &[&str]) {
    let Ok(entries) = std::fs::read_dir(from) else {
        return;
    };
    let _ = cf_e2e::door::make_dir(to);
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() || left.iter().any(|word| name.contains(word)) {
            continue;
        }
        if kind.is_dir() {
            copy_tree(&path, &to.join(&name), left);
        } else {
            let _ = cf_e2e::door::copy_keeping_mode(&path, to.join(&name));
        }
    }
}

/// What Devin's store `file` holds of compaction: how many rows say they were summarized
/// from others (`metadata.summarized_from`), and the `<context_usage>` notes it gave the model,
/// each as its first line says it.
fn devin_summaries_and_notes(file: &Path) -> Result<(usize, Vec<String>)> {
    if !cf_e2e::live::guard::run_owned_devin_store(file) {
        return Err(Error::Daemon(
            "Devin's store is outside the run's own roots".to_owned(),
        ));
    }
    let fail = |cause: rusqlite::Error| Error::Daemon(format!("{}: {cause}", file.display()));
    let db =
        rusqlite::Connection::open_with_flags(file, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(fail)?;
    let summaries: i64 = db
        .query_row(
            "SELECT count(*) FROM message_nodes WHERE json_extract(metadata, '$.summarized_from') IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(fail)?;
    let mut notes = Vec::new();
    let mut read = db
        .prepare(
            "SELECT json_extract(chat_message, '$.content') FROM message_nodes
              WHERE chat_message LIKE '%<context_usage>%' ORDER BY row_id",
        )
        .map_err(fail)?;
    let rows = read
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .map_err(fail)?;
    for row in rows {
        if let Some(text) = row.map_err(fail)? {
            let line: String = text
                .lines()
                .find(|line| line.contains("<context_usage>"))
                .unwrap_or_default()
                .chars()
                .take(60)
                .collect();
            if !notes.contains(&line) {
                notes.push(line);
            }
        }
    }
    Ok((usize::try_from(summaries).unwrap_or(0), notes))
}

/// Devin's own folders in the kit's home, below those its variables name: the run's data
/// folder links the owner's other folders, which are never read here.
fn devin_folders(home: &[(String, String)]) -> Result<Vec<PathBuf>> {
    cf_e2e::live::guard::devin_store_path(cfg!(windows), home)?;
    home.iter()
        .filter(|(name, _)| {
            [
                "XDG_DATA_HOME",
                "XDG_STATE_HOME",
                "XDG_CACHE_HOME",
                "APPDATA",
                "LOCALAPPDATA",
            ]
            .contains(&name.as_str())
        })
        .map(|(_, value)| {
            let base = PathBuf::from(value);
            let folder = base.join("devin");
            if value.is_empty()
                || !cf_e2e::door::is_made_or_below(&base)
                || !cf_e2e::door::is_made_or_below(&folder)
            {
                return Err(Error::Daemon(
                    "Devin's folder is outside the run's own roots".to_owned(),
                ));
            }
            Ok(folder)
        })
        .collect()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on two short Devin turns; cargo xtask live compaction-routes -- devin"]
fn devin_compacts_at_the_thresholds_its_launch_gives() -> Result {
    let log = Log::begin("live compaction-routes devin")?;
    ask::prepare()?;
    let install = machine::installed_for(Harness::Devin, Duration::from_secs(1200))?;
    let model = agents::model(Harness::Devin);
    let home = install.home_outside_a_rig()?;
    let devin = install.command().to_string_lossy().into_owned();
    let turn = |thresholds: &str| -> Result<(cf_e2e::door::Made, PathBuf)> {
        let (scratch, folder) = scratch_folder()?;
        let args = vec![
            devin.clone(),
            "-p".to_owned(),
            LONG_ASK.to_owned(),
            "--model".to_owned(),
            model.name.clone(),
            "--respect-workspace-trust".to_owned(),
            "false".to_owned(),
            "--permission-mode".to_owned(),
            "dangerous".to_owned(),
        ];
        let argv = ask::program(&args, &machine_vars())?;
        let (program, rest) = argv
            .split_first()
            .ok_or_else(|| Error::Daemon("no command line".to_owned()))?;
        let ran = Run::new(program)
            .args(rest)
            .cwd(&folder)
            .inheriting_env()
            .vars(home.clone())
            .var("DEVIN_COMPACTION_THRESHOLDS", thresholds)
            .closed_input()
            .limit(TURN)
            .run()?;
        log.say(format!(
            "devin -p at {thresholds}: exit {:?}; {}",
            ran.code,
            ran.output().lines().last().unwrap_or_default()
        ));
        Ok((scratch, folder))
    };
    // The thresholds the turn is given (`CF_DEVIN_THRESHOLDS`, `8000,8000` unless the run
    // says otherwise: Devin told a turn at `1000,1000` its limit was 5k), and its store read
    // after it: the summaries Devin wrote (`summarized_from` in a row's metadata) and the
    // context notes it gave the model.
    let thresholds = own_var("CF_DEVIN_THRESHOLDS").unwrap_or_else(|| "8000,8000".to_owned());
    let _turn = turn(&thresholds)?;
    let store = cf_e2e::live::guard::devin_store_path(cfg!(windows), &home)?;
    let (summaries, notes) = devin_summaries_and_notes(&store)?;
    log.block(
        &format!(
            "devin {} on {} at {thresholds}",
            install.version(),
            model.name
        ),
        &format!(
            "rows Devin wrote as a summary: {summaries}\ncontext notes the model was given: {}",
            if notes.is_empty() {
                "none".to_owned()
            } else {
                notes.join(" | ")
            }
        ),
    );
    let compacted = summaries;
    let before = 0;
    // What Devin wrote in the run's home, copied out where `CF_EVIDENCE` names a folder: its
    // store and logs, never its login. The folder is its caller's: the copy is made in a new
    // folder of the run's own in it, and a folder of a run before is never written into.
    if let Some(evidence) =
        cf_e2e::live::export_folder(own_var("CF_EVIDENCE").as_deref(), "compaction-routes-devin")?
    {
        log.say(format!(
            "what Devin wrote is copied into {}",
            evidence.display()
        ));
        for (at, root) in devin_folders(&home)?.iter().enumerate() {
            copy_tree(
                root,
                &evidence.join(format!("devin-{at}")),
                &["credentials"],
            );
        }
    }
    install.finish_homes()?;
    assert!(compacted > before, "Devin never compacted at {thresholds}");
    log.passed();
    Ok(())
}

fn watch_muse_owner(home: &Path) -> Result<cf_e2e::live::isolation::Isolation> {
    let watch = cf_e2e::live::isolation::Isolation::muse(
        home,
        &home.join(".config"),
        &home.join(".local/share"),
    );
    watch.vars()?;
    Ok(watch)
}

/// What a Muse session log says of compaction: each record whose payload names a threshold
/// or a compaction, by its payload type and those fields.
fn muse_compaction_lines(data: &Path) -> Result<Vec<String>> {
    if !cf_e2e::door::is_made_or_below(data) {
        return Err(Error::Daemon(
            "Muse's session data is outside the run's own roots".to_owned(),
        ));
    }
    let mut lines = Vec::new();
    for path in files_saying(data, "\"payload_type\"") {
        if path.file_name().and_then(|name| name.to_str()) != Some("session.jsonl") {
            continue;
        }
        let Ok(text) = files::read_string(&path) else {
            continue;
        };
        for record in text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            let kind = record["payload_type"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let payload = record["payload"].to_string();
            if kind.contains("compact") || payload.contains("threshold_tokens") {
                let mut fields = Vec::new();
                for key in [
                    "soft_threshold_tokens",
                    "hard_threshold_tokens",
                    "estimated_prompt_tokens",
                    "tokens_before",
                    "tokens_after",
                    "trigger",
                ] {
                    if let Some(at) = payload.find(&format!("\"{key}\":")) {
                        let value: String = payload[at..]
                            .chars()
                            .take_while(|c| *c != ',' && *c != '}')
                            .collect();
                        fields.push(value);
                    }
                }
                lines.push(format!("{kind} {}", fields.join(" ")));
            }
        }
    }
    Ok(lines)
}

#[test]
#[ignore = "opt-in: spends Muse's allowance on two short turns; cargo xtask live compaction-routes -- muse"]
fn muse_compacts_at_the_soft_threshold_its_launch_gives() -> Result {
    let log = Log::begin("live compaction-routes muse")?;
    ask::prepare()?;
    let home = machine::home()?;
    let owner_watch = watch_muse_owner(&home)?;
    let auth = files::read_string(&home.join(".config/muse/auth.json"))?;
    login_in_keychain(&auth).map_err(Error::Daemon)?;
    let muse = machine::command("muse")?;
    let model = agents::model(Harness::Muse);
    let mut said = Vec::new();
    let mut failed = Vec::new();
    // Each case, its knobs, and whether its short turn must compact: the low limits must; the
    // limit above the model's own window may not, and proves nothing either way.
    for (name, knobs, compacts) in [
        (
            "soft 0.03",
            vec!["context_compaction.soft_threshold=0.03"],
            true,
        ),
        (
            "provider limit 40000",
            vec!["context_compaction.provider_context_limit_tokens=40000"],
            true,
        ),
        (
            "provider limit 900000",
            vec!["context_compaction.provider_context_limit_tokens=900000"],
            false,
        ),
    ] {
        // `CF_ONLY` runs the cases whose name holds its words, and no other.
        if own_var("CF_ONLY").is_some_and(|only| !name.contains(&only)) {
            continue;
        }
        let scratch = Scratch::made("compaction-routes-muse-").map_err(|source| Error::File {
            action: "make a folder in",
            path: std::env::temp_dir(),
            source,
        })?;
        copy_muse_into(&home, scratch.path()).map_err(|source| Error::File {
            action: "copy the login and the model catalog of",
            path: home.join(".config/muse"),
            source,
        })?;
        let (_folder_scratch, folder) = scratch_folder()?;
        let mut args = vec![
            muse.to_string_lossy().into_owned(),
            "exec".to_owned(),
            "--model".to_owned(),
            model.name.clone(),
            "--contributor-mode".to_owned(),
            "off".to_owned(),
            "--workspace".to_owned(),
            folder.to_string_lossy().into_owned(),
        ];
        if let Some(effort) = &model.effort {
            args.extend(["--reasoning-effort".to_owned(), effort.clone()]);
        }
        for knob in knobs {
            args.extend(["-c".to_owned(), knob.to_owned()]);
        }
        args.push(ASK.to_owned());
        let argv = ask::program(&args, &machine_vars())?;
        let (program, rest) = argv
            .split_first()
            .ok_or_else(|| Error::Daemon("no command line".to_owned()))?;
        let ran = Run::new(program)
            .args(rest)
            .cwd(&folder)
            .inheriting_env()
            .vars([
                ("XDG_CONFIG_HOME", scratch.path().join("config")),
                ("XDG_DATA_HOME", scratch.path().join("data")),
                ("XDG_CACHE_HOME", scratch.path().join("cache")),
                ("XDG_STATE_HOME", scratch.path().join("state")),
            ])
            .var("MUSE_NO_AUTO_UPDATE", "1")
            .closed_input()
            .limit(TURN)
            .run()?;
        let lines = muse_compaction_lines(&scratch.path().join("data"))?;
        if ran.code != Some(0) {
            failed.push(format!("{name}: muse exec ended {:?}", ran.code));
        }
        // A compaction Muse began is a record that says what triggered it.
        if compacts && !lines.iter().any(|line| line.contains("\"trigger\":")) {
            failed.push(format!("{name}: no compaction was triggered"));
        }
        said.push(format!(
            "{name}: exit {:?}\n  {}",
            ran.code,
            if lines.is_empty() {
                "no record names a threshold or a compaction".to_owned()
            } else {
                lines.join("\n  ")
            }
        ));
    }
    log.block(&format!("muse on {}", model.name), &said.join("\n"));
    owner_watch.finish()?;
    assert!(failed.is_empty(), "{}", failed.join("; "));
    log.passed();
    Ok(())
}

/// The size the Pi case gives the project: the least the setting takes (`COMPACTION_MIN`).
const PI_SIZE: i64 = 250_000;

/// How many files of some 12,000 tokens the Pi member reads (Pi's tools cut a result at 50 KB):
/// together more than the size, each read after the one before.
const PI_PARTS: usize = 24;

/// How long the Pi member has to finish its task, and then its window to be kept for the
/// compaction (the engine's bound is 300 s), and to close once it is over.
const PI_TASK: Duration = Duration::from_secs(1_800);
const PI_COMPACTION: Duration = Duration::from_secs(330);
const PI_CLOSE: Duration = Duration::from_secs(60);

/// The `tokensBefore` of every `compaction` entry in the sessions Pi kept in `agent_dir`.
fn pi_compactions(agent_dir: &Path) -> Result<Vec<i64>> {
    let mut found = Vec::new();
    let mut folders = vec![agent_dir.join("sessions")];
    while let Some(folder) = folders.pop() {
        if !cf_e2e::door::is_made_or_below(&folder) {
            return Err(Error::Daemon(
                "Pi's sessions are outside the run's own roots".to_owned(),
            ));
        }
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => folders.push(path),
                Ok(kind) if kind.is_file() => {
                    if !cf_e2e::door::is_made_or_below(&path) {
                        return Err(Error::Daemon(
                            "a Pi session is outside the run's own roots".to_owned(),
                        ));
                    }
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    found.extend(text.lines().filter_map(|line| {
                        let entry: Value = serde_json::from_str(line).ok()?;
                        (entry["type"] == "compaction")
                            .then(|| entry["tokensBefore"].as_i64())
                            .flatten()
                    }));
                }
                _ => {}
            }
        }
    }
    Ok(found)
}

/// A compaction with the same pane observed before and after reading its entry. A first
/// observation of an already-written entry cannot prove the window was open at the write.
fn observed_pi_entry(
    seen: &mut Option<Value>,
    before: Option<Value>,
    entries: Vec<i64>,
    after: Option<Value>,
) -> Option<(bool, Vec<i64>)> {
    if entries.is_empty() {
        if before.is_some() && before == after {
            *seen = before;
        } else {
            *seen = None;
        }
        return None;
    }
    let open =
        seen.is_some() && before.as_ref() == seen.as_ref() && after.as_ref() == seen.as_ref();
    Some((open, entries))
}

/// The project at the least size, one task of two dozen reads given to the Pi member, and what
/// came of its window: whether it was open when Pi wrote its `compaction` entry, whether it
/// closed after, and the entries' `tokensBefore`.
fn pi_steps(members: &mut Members, log: &Log) -> Result<(bool, bool, Vec<i64>)> {
    let chosen = members.page(
        "project.compaction",
        json!({ "project": members.project_id(), "compaction": PI_SIZE }),
    )?;
    assert_eq!(chosen["project"]["compaction"], json!(PI_SIZE), "{chosen}");
    let agent = agents::id(Harness::Pi);
    members.add(&agent, &[])?;
    let files: Vec<String> = (1..=PI_PARTS)
        .map(|part| format!("`cat part-{part:02}.txt`"))
        .collect();
    let number = members.give(
        &agent,
        &format!(
            "Run these shell commands one at a time, each only after the one before has finished, never two in one call: {}. Then answer with the single word done.",
            files.join(", ")
        ),
    )?;
    let agent_dir = members
        .var("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Daemon("the kit gave Pi no PI_CODING_AGENT_DIR".to_owned()))?;
    // Start before the task ends: the same pane must be visible on both sides of the
    // first completed entry, and in an earlier look with no entry.
    let mut seen = None;
    let (open_at_entry, entries) = find(
        log,
        "Pi's `compaction` entry",
        PI_TASK + PI_COMPACTION,
        Duration::from_secs(1),
        || {
            let before = pi_open_pane(members, &agent)?;
            let entries = pi_compactions(&agent_dir)?;
            let after = pi_open_pane(members, &agent)?;
            Ok(observed_pi_entry(&mut seen, before, entries, after))
        },
    )?
    .unwrap_or_default();
    find(
        log,
        "the task to be done",
        PI_TASK,
        Duration::from_secs(2),
        || Ok(is_over(&members.task(number)?).then_some(())),
    )?
    .ok_or_else(|| Error::Daemon(format!("T-{number} never ended")))?;
    let closed = find(
        log,
        "its window to close",
        PI_CLOSE,
        Duration::from_secs(1),
        || Ok(members.window_of(&agent)?.is_none().then_some(())),
    )?
    .is_some();
    log.say(format!(
        "compaction entries (tokensBefore): {entries:?}; the window was open when the first came: {open_at_entry}; it closed after: {closed}"
    ));
    Ok((open_at_entry, closed, entries))
}

/// A lane can still name a pane after its host has closed it: verify the pane itself.
fn pi_open_pane(members: &Members, agent: &str) -> Result<Option<Value>> {
    let Some(window) = members.window_of(agent)? else {
        return Ok(None);
    };
    Ok(members.rig().snapshot(&window.pane, 1).map(|_| window.pane))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one task of two dozen calls on Pi's cheap model, and its summary; cargo xtask live compaction-routes -- pi"]
fn pi_member_whose_task_ends_past_the_size_has_its_window_kept_open_for_the_compaction_its_extension_asks_for(
) -> Result {
    let log = Log::begin("live compaction-routes pi")?;
    let line = "These are the notes of the compaction check, a line of filler words to read.\n";
    let mut plan = Plan::new("compaction-routes-pi", members::stand_in_chief())
        .with(members::member(Harness::Pi));
    for part in 1..=PI_PARTS {
        plan = plan.with_file(&format!("part-{part:02}.txt"), line.repeat(600));
    }
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), &log)?;
    let ran = pi_steps(&mut members, &log);
    let ended = members.close();
    let (open_at_entry, closed, entries) = ran?;
    ended?;
    assert!(
        !entries.is_empty(),
        "Pi never wrote a compaction entry: its window was closed under the compaction, or the extension never asked"
    );
    assert!(
        entries.iter().any(|before| *before >= PI_SIZE),
        "no compaction began at the size or past it: {entries:?}"
    );
    assert!(
        open_at_entry,
        "the window was closed before Pi wrote the compaction"
    );
    assert!(closed, "the window was kept past the compaction");
    log.passed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kit_round3_devin_folders_refuse_a_foreign_home() {
        let foreign = tempfile::tempdir().unwrap();
        let env = vec![(
            "XDG_DATA_HOME".to_owned(),
            foreign.path().display().to_string(),
        )];
        assert!(
            devin_folders(&env).is_err(),
            "a foreign Devin folder was accepted"
        );
    }

    #[test]
    fn kit_round3_devin_compaction_store_refuses_a_foreign_database() {
        let foreign = tempfile::tempdir().unwrap();
        let store = foreign.path().join("sessions.db");
        let db = rusqlite::Connection::open(&store).unwrap();
        db.execute_batch("CREATE TABLE message_nodes (metadata TEXT, chat_message TEXT, row_id INTEGER); INSERT INTO message_nodes VALUES ('{\"summarized_from\":1}', '{}', 1);").unwrap();
        drop(db);
        assert!(
            devin_summaries_and_notes(&store).is_err(),
            "a foreign Devin store was opened"
        );
    }

    #[test]
    fn kit_disclosure_muse_compaction_keeps_the_owner_isolation_watch() {
        let home = tempfile::tempdir().unwrap();
        let settings = home.path().join(".config/muse/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "before").unwrap();
        let watch = watch_muse_owner(home.path()).unwrap();
        std::fs::write(&settings, "after").unwrap();
        assert!(
            watch.finish().is_err(),
            "a change to the fabricated owner was missed"
        );
    }

    #[test]
    fn kit_disclosure_muse_oracle_does_not_read_a_foreign_tree() {
        let owner = tempfile::tempdir().unwrap();
        std::fs::write(owner.path().join("foreign.json"), "fabricated").unwrap();
        assert!(
            muse_compaction_lines(owner.path()).is_err(),
            "the oracle accepted a foreign tree"
        );
    }

    #[test]
    fn kit_disclosure_pi_compaction_refuses_foreign_session_root() {
        let foreign = tempfile::tempdir().unwrap();
        std::fs::create_dir(foreign.path().join("sessions")).unwrap();
        std::fs::write(
            foreign.path().join("sessions/s.jsonl"),
            "{\"type\":\"compaction\",\"tokensBefore\":42}\n",
        )
        .unwrap();
        assert!(
            pi_compactions(foreign.path()).is_err(),
            "a foreign Pi session was accepted"
        );
        #[cfg(unix)]
        {
            let run = cf_e2e::door::made("cf-pi-sessions-").unwrap();
            std::os::unix::fs::symlink(
                foreign.path().join("sessions"),
                run.path().join("sessions"),
            )
            .unwrap();
            assert!(
                pi_compactions(run.path()).is_err(),
                "a linked-out Pi session was accepted"
            );
        }
    }

    #[test]
    fn kit_disclosure_pi_observes_a_compaction_before_task_completion() {
        let pane = json!({"id":"worker", "generation":1});
        let mut seen = None;
        assert_eq!(
            observed_pi_entry(&mut seen, Some(pane.clone()), vec![], Some(pane.clone())),
            None
        );
        assert_eq!(
            observed_pi_entry(&mut seen, Some(pane.clone()), vec![250_123], Some(pane)),
            Some((true, vec![250_123]))
        );
        assert_eq!(
            observed_pi_entry(&mut None, None, vec![250_123], None),
            Some((false, vec![250_123]))
        );
    }

    #[test]
    fn kit_disclosure_pi_does_not_reuse_an_observation_after_the_pane_was_gone() {
        let pane = json!({"id":"worker", "generation":1});
        let mut seen = None;
        assert!(
            observed_pi_entry(&mut seen, Some(pane.clone()), vec![], Some(pane.clone())).is_none()
        );
        assert!(observed_pi_entry(&mut seen, None, vec![], None).is_none());
        assert_eq!(
            observed_pi_entry(&mut seen, Some(pane.clone()), vec![250_123], Some(pane)),
            Some((false, vec![250_123]))
        );
    }
}
