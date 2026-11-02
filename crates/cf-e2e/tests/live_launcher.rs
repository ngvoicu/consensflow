//! Live: Claude Code's native updater, and the owner's `~/.local/bin/claude` (T-1082). It is
//! opt-in (`cargo xtask live launcher`): it opens real windows of Claude Code, and it needs
//! it installed, and the network for its control, so it is ignored unless asked for and is no
//! part of `check` or of CI. It spends no quota: no window is sent a message.
//!
//! A live run keeps the person's `HOME` and gives some windows a throwaway `XDG_DATA_HOME` (a
//! seat for Muse, Devin's home). Claude Code's updater keeps its launcher in `$HOME/.local/bin`
//! and its versions in `$XDG_DATA_HOME/claude/versions`: with the two apart it found no version,
//! installed the newest in the throwaway folder, and re-pointed the person's `claude` at it
//! (the alpha.1 Mac release check, T-1068). The kit tells every window to update nothing
//! (`DISABLE_AUTOUPDATER`, `DISABLE_UPDATES`), and a switch is worth what the binary does with
//! it: 2.1.314 and 2.1.316 read both; 2.1.263 read neither (T-751). So this shows it, on the
//! Claude Code the machine has, in two stages:
//!
//! 1. **A pretend owner** (`--stage fake`): a home with a copy of the machine's binary as the
//!    owner's `~/.local/bin/claude`, a throwaway config folder (the onboarding answered, no
//!    login), and the incident's environment: `HOME` the owner's, `XDG_DATA_HOME` the run's.
//!    *The control* is a rig whose case took the switches out: the updater must re-point the
//!    link within a minute, and the rig that ends must fail and name it, or the run proves
//!    nothing. *Then the kit's own*: a window held as long, whose link must be as it was and
//!    whose `XDG_DATA_HOME` must hold no `claude` (no version was installed) and whose rig must
//!    end clean.
//! 2. **The owner's own** (`--stage real`, only after stage 1 held): one window of the real
//!    Claude Code on the owner's login and home, with the incident's environment, held as
//!    long, whose launcher the guard hashed before and looks at after.
//!
//! ```text
//! cargo xtask live launcher                  both stages
//! cargo xtask live launcher --stage fake     the pretend owner's alone (no login needed)
//! ```

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::Open;
use cf_e2e::live::{self, ask, Harness, Installed, Log};
use cf_e2e::process::Run;
use cf_e2e::rig::Rig;
use cf_e2e::{files, Error, Result};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no window
/// of this test runs it, but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// How long a window is held after it drew its prompt for the updater to have run: it ran ten
/// seconds after the screen mounted on the machine of the run that found it (2.1.314, 2.1.316).
const HELD: Duration = Duration::from_secs(45);

/// How long the real window's login must last: the stage takes about three minutes (the window
/// opens in under a minute and is held `HELD`), so a login that lasts less than the kit's two
/// hours is copied all the same, and never refreshed by a window that ends long before it does.
const REAL_LASTS: Duration = Duration::from_secs(20 * 60);

/// How long the control waits for the updater to show that it runs at all.
const CONTROL: Duration = Duration::from_secs(90);

/// An io error as the run's, saying what it was doing to what.
fn io<'a>(action: &'a str, path: &'a Path) -> impl FnOnce(std::io::Error) -> Error + 'a {
    move |failed| Error::Daemon(format!("could not {action} {}: {failed}", path.display()))
}

/// What the owner's `claude` link is: the target it reads.
fn points(link: &Path) -> Result<PathBuf> {
    fs::read_link(link).map_err(io("read the link", link))
}

/// A copy of the file `from` at `to`, a clone where the system makes one (a Claude Code is
/// three hundred megabytes).
fn copy(from: &Path, to: &Path) -> Result {
    files::make_dir(to.parent().unwrap_or(Path::new(".")))?;
    let cloned = Run::new("/bin/cp")
        .arg("-c")
        .arg(from)
        .arg(to)
        .run()
        .is_ok_and(|ran| ran.code == Some(0));
    if !cloned {
        fs::copy(from, to).map_err(io("copy", from))?;
    }
    Ok(())
}

/// What a stage of the pretend owner came to.
struct Came {
    /// Whether the owner's link was re-pointed within the time.
    moved: bool,
    /// Whether the run's `XDG_DATA_HOME` came to hold a `claude` folder.
    installed_in_the_run: bool,
    /// What the rig that ended said.
    closed: Result,
}

/// A pretend owner, a rig on the incident's environment, a Claude window held on it, and what
/// came of it: with the kit's switches (`told`), or with the case having taken them out.
fn fake_owner(log: &Log, binary: &Path, told: bool) -> Result<Came> {
    let owner = tempfile::tempdir().map_err(io("make a folder", Path::new("tmp")))?;
    let home = owner.path();
    let name = binary
        .file_name()
        .ok_or_else(|| Error::Daemon("the machine's Claude has no name".to_owned()))?;
    let version = home.join(".local/share/claude/versions").join(name);
    copy(binary, &version)?;
    let link = home.join(".local/bin/claude");
    files::make_dir(link.parent().unwrap_or(home))?;
    symlink(&version, &link).map_err(io("link", &link))?;
    // The run's config folder: the onboarding answered, the live folders trusted, no login.
    let config = tempfile::tempdir().map_err(io("make a folder", Path::new("tmp")))?;
    files::write(
        &config.path().join(".claude.json"),
        r#"{"hasCompletedOnboarding":true,"hasSeenAutoDefaultNotice":true}"#,
    )?;
    live::machine::seed_claude_trust(config.path(), home)?;
    // The XDG folder a seat for Muse gives every window of a rig.
    let data = tempfile::tempdir().map_err(io("make a folder", Path::new("tmp")))?;
    let fake = Installed::at(Harness::Claude, &link, home);
    let mut rig =
        live::rig_with_claude_config(std::slice::from_ref(&fake), FAKE_AGENT, config.path())
            .var("XDG_DATA_HOME", data.path().to_string_lossy());
    if !told {
        rig = rig.unset("DISABLE_AUTOUPDATER").unset("DISABLE_UPDATES");
    }
    let rig = Rig::start(rig)?;
    let was = points(&link)?;
    let who = if told { "kit" } else { "control" };
    let window = Open::new(&rig, &fake, "launcher", "launcher-fake").open_telling(log, who)?;
    let within = if told { HELD } else { CONTROL };
    let began = std::time::Instant::now();
    let moved = live::until(within, Duration::from_secs(1), || {
        (points(&link).ok()? != was).then_some(())
    })
    .is_some();
    log.say(format!(
        "{who}: {} s after the window drew its prompt the owner's link {}",
        began.elapsed().as_secs(),
        if moved {
            "was re-pointed"
        } else {
            "was as it was"
        }
    ));
    drop(window);
    let installed_in_the_run = data.path().join("claude").exists();
    let closed = rig.close();
    Ok(Came {
        moved,
        installed_in_the_run,
        closed,
    })
}

/// One window of the real Claude Code, on the owner's own home and login, in the incident's
/// environment, held as long as the updater needs: the guard of the rig hashed the owner's
/// launcher before it started and looks at it as the rig ends.
fn owner_own(log: &Log) -> Result<(PathBuf, bool, Result)> {
    let real = log.found_lasting(Harness::Claude, REAL_LASTS)?;
    let link = real.home().join(".local/bin/claude");
    let was = points(&link)?;
    log.say(format!(
        "the owner's {} reads {} (the guard puts it in its words if it moves)",
        link.display(),
        was.display()
    ));
    let data = tempfile::tempdir().map_err(io("make a folder", Path::new("tmp")))?;
    let rig = Rig::start(
        live::rig(std::slice::from_ref(&real), FAKE_AGENT)
            .var("XDG_DATA_HOME", data.path().to_string_lossy()),
    )?;
    let window = Open::new(&rig, &real, "launcher", "launcher-real").open_telling(log, "owner")?;
    let moved = live::until(HELD, Duration::from_secs(1), || {
        (points(&link).ok()? != was).then_some(())
    })
    .is_some();
    log.say(format!(
        "owner: {} s after the window drew its prompt the owner's link {}",
        HELD.as_secs(),
        if moved {
            "was re-pointed"
        } else {
            "was as it was"
        }
    ));
    drop(window);
    let installed_in_the_run = data.path().join("claude").exists();
    Ok((was, moved || installed_in_the_run, rig.close()))
}

#[test]
#[ignore = "opt-in: opens windows of the machine's Claude Code, spends no quota, needs the network for its control; cargo xtask live launcher"]
fn claude_code_honours_the_switches_the_kit_sets_and_a_real_window_leaves_the_owners_launcher_as_it_was(
) -> Result {
    let log = Log::begin("live launcher")?;
    ask::prepare()?;
    let stage = live::option("stage").unwrap_or_else(|| "both".to_owned());
    if !["both", "fake", "real"].contains(&stage.as_str()) {
        return Err(Error::Daemon(format!(
            "--stage is fake, real or both, not {stage}"
        )));
    }
    let command = live::command("claude")?;
    let version = Installed::at(Harness::Claude, &command, Path::new("/")).version();
    let binary = fs::canonicalize(&command).map_err(io("read the link", &command))?;
    log.say(format!("claude is {} ({version})", binary.display()));
    let mut checks = Vec::new();

    if stage != "real" {
        let control = fake_owner(&log, &binary, false)?;
        checks.push(Check::new(
            "claude",
            "the control: with the switches out, the updater re-points the link of a pretend owner",
            control.moved && control.closed.is_err(),
            if control.moved {
                "it did, and the rig that ended said so"
            } else {
                "it did not within the time: this run proves nothing (the updater may have moved or changed)"
            },
        ));
        let kit = fake_owner(&log, &binary, true)?;
        checks.push(Check::new(
            "claude",
            "with the kit's switches the updater leaves the pretend owner's link alone and installs nothing",
            !kit.moved && !kit.installed_in_the_run && kit.closed.is_ok(),
            format!(
                "link moved: {}; installed in the run's XDG folder: {}; rig ended: {}",
                kit.moved,
                kit.installed_in_the_run,
                kit.closed
                    .as_ref()
                    .map_or_else(ToString::to_string, |()| "clean".to_owned())
            ),
        ));
    }
    let honoured = checks.iter().all(|check| check.ok);
    // The owner's own launcher is risked only by a Claude Code that was shown to honour the
    // switches on this machine, in this run.
    if stage != "fake" && (stage == "real" || honoured) {
        let (was, changed, closed) = owner_own(&log)?;
        checks.push(Check::new(
            "claude",
            "a real window on the owner's login, in the incident's environment, leaves the owner's launcher as it was",
            !changed && closed.is_ok(),
            format!(
                "the launcher read {}; changed: {changed}; rig ended: {}",
                was.display(),
                closed
                    .as_ref()
                    .map_or_else(ToString::to_string, |()| "clean".to_owned())
            ),
        ));
    } else if stage != "fake" {
        checks.push(Check::failed(
            "claude",
            "a real window on the owner's login",
            "not run: the switches were not shown to be honoured, and the owner's launcher is not risked",
        ));
    }
    report::conclude(&log, "what the owner's launcher came to", &checks)
}
