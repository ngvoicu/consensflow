//! Live: Claude Code's offer to install an LSP plugin in a member's window, mid-turn, is told
//! to the human and to the task's giver (T-1183). It is opt-in (`cargo xtask live
//! harness-dialogs`): it spends the machine's quota on one short task of a real Claude Code,
//! and needs it installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! The report it answers (field, 3.0.6, 2026-10-26): Claude Code offered a worker's window to
//! install an LSP plugin, and nobody heard of it. Claude Code 2.1.317 offers it once a turn
//! writes a file of an extension a known marketplace has an LSP plugin for, whose language
//! server is on the `PATH`, unless its own config says the offers are off
//! (`lspRecommendationDisabled`, or five of them left unanswered,
//! `lspRecommendationIgnoredCount`): no setting of a launch's turns it off, so the run shows it
//! told of. The run's config folder (the kit's own, `cf_e2e::live::login`) is given the
//! marketplace the person's Claude Code knows, copied (`claude-plugins-official`, which has
//! `clangd-lsp` for `.c` files); its server, `clangd`, is in `/usr/bin` on a Mac with the
//! developer tools, which is on the `PATH` a rig gives its windows (a server in a folder of
//! the person's own, `~/.cargo/bin/rust-analyzer`, is not, and Claude Code offers no plugin
//! whose server it cannot find: the first run, 2026-11-03, saw none for a `.rs` file).
//!
//! The run is a real Claude Code as the one member of a project whose chief is the stand-in.
//! The check, as the chief, gives it a task that writes a `.rs` file and then waits a minute in
//! its shell, so that the turn is still at work while the offer is open, and looks every half
//! second at the window's screen as the pane host draws it, at Claude Code's status file and
//! at the notes. It holds that the offer was drawn, that the human and the chief were told once
//! with its question, that it was then declined ("No, not now", T-1192) well before its 30 s,
//! that the task came to its result, and that nothing of the offer was written: in the run's
//! own `.claude.json` (no ignored count), or in the keys of the offers in the owner's own
//! `~/.claude.json`, whose digest the run says before and after. What the
//! screen showed, and what Claude Code's status said meanwhile (`busy`, in a turn at work: only
//! the screen shows the offer), are in the log, word for word, for the reader's tests.
//!
//! ```text
//! cargo xtask live harness-dialogs
//! ```

use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{member, stand_in_chief, Account, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, text, Harness, Log};
use cf_e2e::process::own_var;
use cf_e2e::{Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The question Claude Code's offer asks.
const QUESTION: &str = "Would you like to install this LSP plugin?";

/// The marketplace the run's config folder is given, from the person's own.
const MARKETPLACE: &str = "claude-plugins-official";

/// How long the run waits for the task's result.
const WORK: Duration = Duration::from_secs(240);

/// How often the screen, the status and the notes are looked at.
const LOOK: Duration = Duration::from_millis(500);

/// What the worker is asked.
const BRIEF: &str = "Use your file-writing tool (not the shell) to create the file probe_dialog.c in the current folder, holding exactly: int main(void) { return 0; }\nThen run this exact command with your shell tool and wait for it to finish: sleep 60\nThen reply with exactly one line: DONE";

/// Gives the run's Claude config folder `config` the marketplace the person's Claude Code
/// knows, copied, and names it there as Claude Code names a marketplace it added.
fn seed_marketplace(config: &Path) -> Result {
    let home = own_var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    seed_marketplace_from(&home, config)
}

/// [`seed_marketplace`] from the marketplace the Claude Code of `home` knows: read there, written
/// only in `config`, and only where the run made it (the door refuses another folder, a caller's
/// among them).
fn seed_marketplace_from(home: &Path, config: &Path) -> Result {
    let from = home.join(".claude/plugins/marketplaces").join(MARKETPLACE);
    let plugins = config.join("plugins");
    let to = plugins.join("marketplaces").join(MARKETPLACE);
    copy_tree(&from, &to)?;
    let known = json!({
        MARKETPLACE: {
            "source": { "source": "github", "repo": "anthropics/claude-plugins-official" },
            "installLocation": to.display().to_string(),
            "lastUpdated": "2026-11-03T00:00:00.000Z",
        }
    });
    cf_e2e::door::write(
        &plugins.join("known_marketplaces.json"),
        format!("{known}\n"),
    )
}

/// Copies the folder `from` into `to`, its files and folders, links left out.
fn copy_tree(from: &Path, to: &Path) -> Result {
    cf_e2e::door::make_dir(to)?;
    let entries = fs::read_dir(from)
        .map_err(|failed| Error::Daemon(format!("{}: {failed}", from.display())))?;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            cf_e2e::door::copy(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// What Claude Code's statuses in the run's config folder say: each window's word, and what
/// it waits on where it says it waits.
fn statuses(config: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(config.join("sessions")) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| fs::read_to_string(entry.ok()?.path()).ok())
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|status| match status["waitingFor"].as_str() {
            Some(why) => format!("{} ({why})", text(&status["status"])),
            None => text(&status["status"]),
        })
        .collect()
}

/// The notes of a dialog in a window among `messages`.
fn dialog_notes(messages: &[Value]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message["kind"] == "note")
        .filter_map(|message| message["body"].as_str())
        .filter(|body| body.contains("'s window shows a dialog of its harness"))
        .map(str::to_owned)
        .collect()
}

/// What the looks found.
#[derive(Default)]
struct Seen {
    /// The screen the offer was first seen on, as the pane host drew it.
    offer_screen: Option<String>,
    /// When the offer was first and last seen, from the task's being given.
    offer_from: Option<Duration>,
    offer_until: Option<Duration>,
    /// What the engine's tail, the last twelve lines of the screen, showed while the offer was on
    /// it, each change and when (T-1261: the tail slides when the spinner under the offer goes).
    shapes: Vec<(Duration, String)>,
    /// What Claude Code's statuses said while the offer was on the screen, each change.
    said: Vec<String>,
    /// When the human was first told of a dialog.
    told_at: Option<Duration>,
}

/// The worker given its task and looked at until its result, or [`WORK`] is up.
fn observe(members: &Members, agent: &str, log: &Log) -> Result<Account> {
    let mut case = Account::new();
    let config = members.claude_config();
    let number = members.give(agent, BRIEF)?;
    let started = Instant::now();
    log.say(format!("T-{number} is given"));
    let mut seen = Seen::default();
    let mut state = String::new();
    while started.elapsed() < WORK {
        let now = started.elapsed();
        state = members
            .task(number)
            .map(|task| text(&task["state"]))
            .unwrap_or_default();
        if let Ok(Some(window)) = members.window_of(agent) {
            if let Some(snapshot) = members.snapshot(&window.pane, 60) {
                let lines: Vec<String> = snapshot["tail"]
                    .as_array()
                    .map(|lines| lines.iter().map(text).collect())
                    .unwrap_or_default();
                if lines.iter().any(|line| line.contains(QUESTION)) {
                    seen.offer_from.get_or_insert(now);
                    seen.offer_until = Some(now);
                    seen.offer_screen.get_or_insert_with(|| lines.join("\n"));
                    let tail = lines[lines.len().saturating_sub(12)..].join("\n");
                    if seen.shapes.last().map(|(_, shape)| shape) != Some(&tail) {
                        seen.shapes.push((now, tail));
                    }
                }
            }
        }
        if seen.offer_from.is_some() && seen.offer_until == Some(now) {
            let said = statuses(&config).join(", ");
            if seen.said.last() != Some(&said) {
                log.say(format!(
                    "+{:.1} s: Claude Code says {said}",
                    now.as_secs_f64()
                ));
                seen.said.push(said);
            }
        }
        if seen.told_at.is_none() && !dialog_notes(&members.inbox(Some("human"))?).is_empty() {
            seen.told_at = Some(now);
            log.say(format!("+{:.1} s: the human is told", now.as_secs_f64()));
        }
        if state == "done" || state == "failed" {
            break;
        }
        sleep(LOOK);
    }
    if let Some(screen) = &seen.offer_screen {
        log.block("the offer, as the pane host drew it", screen);
    }
    for (at, tail) in &seen.shapes {
        log.block(
            &format!(
                "the engine's tail, its last twelve lines, as it read at +{:.1} s",
                at.as_secs_f64()
            ),
            tail,
        );
    }
    let human = dialog_notes(&members.inbox(Some("human"))?);
    let chief = dialog_notes(&members.inbox(Some(members.chief()))?);
    for note in &human {
        log.block("told the human", note);
    }
    case.check(
        "Claude Code drew its offer to install an LSP plugin",
        seen.offer_from.is_some(),
        format!("from {:?} to {:?}", seen.offer_from, seen.offer_until),
    );
    // Claude Code 2.1.317 said `busy` in a turn at work with the offer open (the second run,
    // 2026-11-03), so the screen alone tells of it then: what it says is the run's finding.
    case.say(format!(
        "Claude Code's statuses while the offer showed: {:?}",
        seen.said
    ));
    case.check(
        "the human was told once, with the question",
        human.len() == 1 && human[0].contains(QUESTION),
        format!("{human:?}, at {:?}", seen.told_at),
    );
    case.check(
        "the chief, who gave the task, was told the same once",
        chief == human,
        format!("{chief:?}"),
    );
    // Since T-1192 the offer, on Claude Code's list of safe declines, is declined after the
    // note ("No, not now", by its digit): it goes well before its 30 s, the note says so, and
    // nothing of it is written, in the run's own config or in the owner's.
    case.check(
        "the note says the offer is declined, choosing \"No, not now\"",
        human.first().is_some_and(|note| {
            note.contains(
                "ConsensFlow declines it for the window now, choosing \u{201c}No, not now\u{201d}",
            )
        }),
        format!("{human:?}"),
    );
    let open_for = seen
        .offer_from
        .zip(seen.offer_until)
        .map(|(from, until)| until - from);
    case.check(
        "the offer went before its 30 s were up: declined, not closed by itself",
        open_for.is_some_and(|open| open < Duration::from_secs(25)),
        format!("open for {open_for:?}"),
    );
    case.check("the task came to its result", state == "done", state);
    let run = lsp_keys(&config.join(".claude.json"));
    case.check(
        "the run's own .claude.json holds nothing of the offer: no ignored count, no plugin never offered, offers not turned off",
        run.iter().all(|(_, value)| value.is_null() || *value == json!(0)),
        format!("{run:?}"),
    );
    let installed = config.join("plugins").join("installed_plugins.json");
    case.check(
        "no plugin was installed in the run's config",
        !installed.exists()
            || fs::read_to_string(&installed).is_ok_and(|text| !text.contains("lsp")),
        format!("{}", installed.display()),
    );
    Ok(case)
}

/// The keys Claude Code writes of its LSP offers in a `.claude.json`, with their values (null
/// where it holds none), and of its plugin hints.
fn lsp_keys(file: &Path) -> Vec<(&'static str, Value)> {
    let config = fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);
    [
        "lspRecommendationIgnoredCount",
        "lspRecommendationNeverPlugins",
        "lspRecommendationDisabled",
        "claudeCodeHints",
    ]
    .into_iter()
    .map(|key| (key, config[key].clone()))
    .collect()
}

/// The owner's `~/.claude.json`: its digest and its keys of the LSP offers. The owner's own
/// Claude Code sessions rewrite the rest of it all day, so the digest may move with them; the
/// keys of the offers move only with an offer answered or left to close.
fn owners_config() -> (String, Vec<(&'static str, Value)>) {
    let file = own_var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default()
        .join(".claude.json");
    let digest = fs::read(&file)
        .map(|bytes| {
            Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .unwrap_or_default();
    (digest, lsp_keys(&file))
}

/// The run.
fn run(log: &Log) -> Result<Account> {
    let plan = Plan::new("harness-dialogs", stand_in_chief()).with(member(Harness::Claude));
    let agent = plan.roster[1]["id"].as_str().unwrap_or_default().to_owned();
    let (digest_before, keys_before) = owners_config();
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    seed_marketplace(&members.claude_config())?;
    members.add(&agent, &[])?;
    let seen = observe(&members, &agent, log);
    let closed = members.close();
    let mut case = seen?;
    case.closed(closed);
    let (digest_after, keys_after) = owners_config();
    case.say(format!(
        "the owner's ~/.claude.json: sha256 {digest_before} before, {digest_after} after ({})",
        if digest_before == digest_after {
            "the same"
        } else {
            "moved: the owner's own sessions write it too"
        }
    ));
    case.check(
        "the owner's ~/.claude.json keys of the LSP offers are as they were",
        keys_before == keys_after,
        format!("before {keys_before:?}, after {keys_after:?}"),
    );
    Ok(case)
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of a real Claude Code; cargo xtask live harness-dialogs"]
fn claude_codes_offer_to_install_an_lsp_plugin_mid_turn_is_told_to_the_human_and_the_chief(
) -> Result {
    let log = Log::begin("live harness dialogs")?;
    let version = live::version_of(Harness::Claude);
    let case = run(&log).unwrap_or_else(|failed| {
        let mut case = Account::new();
        case.fail(&failed);
        case
    });
    log.block(
        &format!(
            "== claude {version}: {}",
            if case.held() { "ok" } else { "FAILED" }
        ),
        &case.text(),
    );
    let checks = vec![Check::new(
        "claude",
        "the offer is told to the human and the chief",
        case.held(),
        version,
    )];
    report::conclude(&log, "what the offer came to", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_e2e::live::layout::Layout;

    /// A person's home with the marketplace their Claude Code knows, below the layout.
    fn owners_home(layout: &mut Layout) -> std::path::PathBuf {
        let home = layout.home().join("owner");
        let marketplace = home.join(".claude/plugins/marketplaces").join(MARKETPLACE);
        let files = [
            ("README.md", "the marketplace's readme"),
            ("plugins/lsp/plugin.json", "{\"name\":\"lsp\"}"),
        ];
        layout.plant_tree(&marketplace, &files).unwrap();
        home
    }

    #[test]
    fn the_marketplace_is_copied_into_the_config_the_run_made_and_the_owners_is_read_only() {
        let mut layout = Layout::new().unwrap();
        let home = owners_home(&mut layout);
        let own = cf_e2e::door::made("live-harness-dialogs-test-").unwrap();
        seed_marketplace_from(&home, own.path()).unwrap();
        let copied = own.path().join("plugins/marketplaces").join(MARKETPLACE);
        assert_eq!(
            fs::read_to_string(copied.join("plugins/lsp/plugin.json")).unwrap(),
            "{\"name\":\"lsp\"}"
        );
        let known = fs::read_to_string(own.path().join("plugins/known_marketplaces.json")).unwrap();
        assert!(known.contains(MARKETPLACE), "{known}");
        // The owner's marketplace is as it was, byte for byte.
        layout.untouched().unwrap();
        cf_e2e::door::again_under_audit("tests::the_marketplace_is_copied_into_the_config_the_run_made_and_the_owners_is_read_only").unwrap();
    }

    #[test]
    fn a_claude_config_a_caller_named_is_never_the_one_the_marketplace_is_written_in() {
        let mut layout = Layout::new().unwrap();
        let home = owners_home(&mut layout);
        // A caller's config folder, with files in it, that no run made: a person's own.
        let callers = tempfile::tempdir().unwrap();
        let (settings, record) = (
            callers.path().join("settings.json"),
            callers.path().join(".claude.json"),
        );
        fs::write(&settings, "{\"theme\":\"dark\"}").unwrap();
        fs::write(&record, "{\"projects\":{}}").unwrap();
        let refused = seed_marketplace_from(&home, callers.path());
        assert!(refused.is_err(), "a caller's config folder was written in");
        assert!(!callers.path().join("plugins").exists());
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "{\"theme\":\"dark\"}"
        );
        assert_eq!(fs::read_to_string(&record).unwrap(), "{\"projects\":{}}");
        layout.untouched().unwrap();
        cf_e2e::door::again_under_audit(
            "tests::a_claude_config_a_caller_named_is_never_the_one_the_marketplace_is_written_in",
        )
        .unwrap();
    }
}
