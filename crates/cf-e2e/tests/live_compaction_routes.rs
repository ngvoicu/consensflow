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
//! Each case runs the harness on a home of the run's own (the kit's [`Isolation`] for Codex
//! and Devin; for Muse a copy of `auth.json` and the model catalog, the login being the
//! keychain's, as `live muse-reads` does) with the knob set low, so that a short turn crosses
//! it, and reads the harness's own record for a compaction:
//!
//! - `codex`: a turn that reads a file, then `exec resume` of the same thread; its rollout
//!   must say `compacted`.
//! - `devin`: one turn at `1000,1000`, whose store must hold a summary; one at thresholds no
//!   model takes, whose log must say Devin ignored them.
//! - `muse`: one turn at a soft threshold of 0.03; one at a provider limit of 40,000 tokens;
//!   one at 900,000, above the model's own limit; the session logs say the thresholds in
//!   tokens and what triggered each compaction (`CF_ONLY=<words>` runs one of them).
//!
//! ```text
//! cargo xtask live compaction-routes -- codex
//! cargo xtask live compaction-routes -- devin
//! cargo xtask live compaction-routes -- muse
//! ```
//!
//! [`Isolation`]: cf_e2e::live::isolation::Isolation

use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::guard::{copy_muse_into, login_in_keychain, Scratch};
use cf_e2e::live::{agents, ask, codex, machine, Harness, Log};
use cf_e2e::process::{own_var, Run};
use cf_e2e::{files, rollout, Error, Result};
use serde_json::Value;

/// How long one headless turn may take.
const TURN: Duration = Duration::from_secs(300);

/// What a turn is asked: read a file of the case's own whose words alone cross the knob.
const ASK: &str = "Run `cat notes.txt` with your shell tool, then run `wc -l notes.txt`, then answer with the single word done.";

/// A file whose words are some 12,000 tokens.
fn notes(folder: &Path) -> Result {
    let line = "These are the notes of the compaction check, a line of filler words to read.\n";
    files::write(&folder.join("notes.txt"), line.repeat(600))
}

/// A folder of the case's own, by the path it resolves to (Codex and Devin know a folder so).
fn scratch_folder() -> Result<(tempfile::TempDir, PathBuf)> {
    let scratch = tempfile::tempdir().map_err(|source| Error::File {
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
    // Every MCP server of the owner's is switched off for the run, as the evals do.
    words.extend(codex::isolation(&codex::mcp_servers(install.command())?)?);
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
    let mut second = vec![
        codex_program.clone(),
        "exec".to_owned(),
        "resume".to_owned(),
        "--last".to_owned(),
    ];
    second.extend(words);
    second.push("Answer with the single word again.".to_owned());
    let ran = run(second)?;
    log.say(format!("codex exec resume: exit {:?}", ran.code));
    let rollouts = rollout::rollouts(&codex_home);
    let mut compacted = 0;
    for path in &rollouts {
        let text = rollout::read(path)?;
        compacted += rollout::records(&text)
            .iter()
            .filter(|record| record["type"] == "compacted")
            .count();
    }
    log.block(
        &format!("codex {} on {}", install.version(), model.name),
        &format!(
            "{} rollout(s); `compacted` records: {compacted}",
            rollouts.len()
        ),
    );
    install.finish_homes()?;
    assert!(compacted > 0, "Codex never compacted at a limit of 2000");
    Ok(())
}

/// Copies every file below `from` into `to`, but those whose name holds a word of `left`.
fn copy_tree(from: &Path, to: &Path, left: &[&str]) {
    let Ok(entries) = std::fs::read_dir(from) else {
        return;
    };
    let _ = std::fs::create_dir_all(to);
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
            let _ = std::fs::copy(&path, to.join(&name));
        }
    }
}

/// Devin's own folders in the kit's home, below those its variables name: the run's data
/// folder links the owner's other folders, which are never read here.
fn devin_folders(home: &[(String, String)]) -> Vec<PathBuf> {
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
        .map(|(_, value)| PathBuf::from(value).join("devin"))
        .collect()
}

#[test]
#[ignore = "opt-in: spends the machine's quota on two short Devin turns; cargo xtask live compaction-routes -- devin"]
fn devin_compacts_at_the_thresholds_its_launch_gives_and_ignores_ones_no_model_takes() -> Result {
    let log = Log::begin("live compaction-routes devin")?;
    ask::prepare()?;
    let install = machine::installed_for(Harness::Devin, Duration::from_secs(1200))?;
    let model = agents::model(Harness::Devin);
    let home = install.home_outside_a_rig()?;
    let devin = install.command().to_string_lossy().into_owned();
    let turn = |thresholds: &str| -> Result<(tempfile::TempDir, PathBuf)> {
        let (scratch, folder) = scratch_folder()?;
        let args = vec![
            devin.clone(),
            "-p".to_owned(),
            ASK.to_owned(),
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
    let summaries = |words: &str| -> usize {
        devin_folders(&home)
            .iter()
            .map(|root| files_saying(root, words).len())
            .sum()
    };
    let before = summaries("file_compactor");
    let _low = turn("1000,1000")?;
    let compacted = summaries("file_compactor");
    let _high = turn("4000000000,4000000000")?;
    let ignored: Vec<PathBuf> = devin_folders(&home)
        .iter()
        .flat_map(|root| files_saying(root, "Ignoring invalid compaction thresholds"))
        .collect();
    log.block(
        &format!("devin {} on {}", install.version(), model.name),
        &format!(
            "files holding a compactor's summary: {before} before, {compacted} after the turn at 1000,1000\nfiles that say Devin ignored the thresholds after the turn at 4000000000: {}",
            ignored
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );
    // What Devin wrote in the run's home, copied out where `CF_EVIDENCE` names a folder: its
    // store and logs, never its login.
    if let Some(evidence) = own_var("CF_EVIDENCE") {
        for (at, root) in devin_folders(&home).iter().enumerate() {
            copy_tree(
                root,
                &Path::new(&evidence).join(format!("devin-{at}")),
                &["credentials"],
            );
        }
    }
    install.finish_homes()?;
    assert!(compacted > before, "Devin never compacted at 1000,1000");
    Ok(())
}

/// Every file below `root` with a digest of its bytes, by name: what a run changed of the
/// owner's folder is said by name, never by what it holds.
fn digest(root: &Path) -> Vec<(PathBuf, u64)> {
    use std::hash::{Hash, Hasher};
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                bytes.hash(&mut hasher);
                found.push((path, hasher.finish()));
            }
        }
    }
    found.sort();
    found
}

/// What a Muse session log says of compaction: each record whose payload names a threshold
/// or a compaction, by its payload type and those fields.
fn muse_compaction_lines(data: &Path) -> Vec<String> {
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
    lines
}

#[test]
#[ignore = "opt-in: spends Muse's allowance on two short turns; cargo xtask live compaction-routes -- muse"]
fn muse_compacts_at_the_soft_threshold_its_launch_gives() -> Result {
    let log = Log::begin("live compaction-routes muse")?;
    ask::prepare()?;
    let home = machine::home()?;
    let auth = files::read_string(&home.join(".config/muse/auth.json"))?;
    login_in_keychain(&auth).map_err(Error::Daemon)?;
    let muse = machine::command("muse")?;
    let owner = home.join(".config/muse");
    let before = digest(&owner);
    let model = agents::model(Harness::Muse);
    let mut said = Vec::new();
    for (name, knobs) in [
        ("soft 0.03", vec!["context_compaction.soft_threshold=0.03"]),
        (
            "provider limit 40000",
            vec!["context_compaction.provider_context_limit_tokens=40000"],
        ),
        (
            "provider limit 900000",
            vec!["context_compaction.provider_context_limit_tokens=900000"],
        ),
    ] {
        // `CF_ONLY` runs the cases whose name holds its words, and no other.
        if own_var("CF_ONLY").is_some_and(|only| !name.contains(&only)) {
            continue;
        }
        let scratch = Scratch::at(std::env::temp_dir().join(format!(
            "compaction-routes-muse-{}-{}",
            std::process::id(),
            said.len()
        )));
        copy_muse_into(&home, &scratch.0).map_err(|source| Error::File {
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
                ("XDG_CONFIG_HOME", scratch.0.join("config")),
                ("XDG_DATA_HOME", scratch.0.join("data")),
                ("XDG_CACHE_HOME", scratch.0.join("cache")),
                ("XDG_STATE_HOME", scratch.0.join("state")),
            ])
            .var("MUSE_NO_AUTO_UPDATE", "1")
            .closed_input()
            .limit(TURN)
            .run()?;
        let lines = muse_compaction_lines(&scratch.0.join("data"));
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
    assert_eq!(
        digest(&owner),
        before,
        "the run changed a file of the owner's ~/.config/muse"
    );
    Ok(())
}
