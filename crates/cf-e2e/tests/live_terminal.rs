//! Live, on Windows: the app's terminal as Devin sees it. It is opt-in (`cargo
//! xtask live terminal`, `npm run live:terminal`) and runs on a Windows machine
//! with Devin installed and logged in, so it is ignored unless asked for and is
//! no part of `check` or of CI. Nothing is sent, no model is asked.
//!
//! The pane host names its terminal to every program (`TERM_PROGRAM=ConsensFlow`);
//! before it did, Devin took the pane for Windows' console host and opened with
//! "Windows Console Host (conhost) has limited support. Switch to Windows
//! Terminal or Git Bash for the best experience" (2026-10-05). A window opened as
//! the app opens one must not say it; one opened with the name taken away must, or
//! this test could not see it. And the console host under each window is
//! Microsoft's `OpenConsole.exe` the app ships beside its pane host (`cargo xtask
//! conpty`), not the system's conhost: one of those running is in the folder the
//! pane host is in.
//!
//! ```text
//! npm run windows -- --host <ssh host> --build -- cargo xtask live terminal
//! ```
//!
//! (`--build` makes the pane host and puts the console host beside it, as the app
//! has it.) The run fails when the app's window warns, or the one with no terminal
//! named does not, or a window's console host is not the shipped one.
//!
//! The test is for Windows: on another machine it is a test that fails, saying so,
//! and not one that passes having looked at nothing. What it judges is in plain
//! functions that are tested on every platform.

use cf_e2e::Result;

/// What Devin says when it takes the pane for Windows' console host.
const WARNING: &str = "Windows Console Host (conhost) has limited support";

/// A window to open, and what it must say of the terminal.
struct Case {
    name: &'static str,
    /// The variables taken from the window, which the pane host names its terminal in.
    drop_env: &'static [&'static str],
    /// Whether Devin must warn of the console host there.
    warns: bool,
}

/// The window as the app opens it, which must not warn, and the same with its
/// terminal's name taken away, which must: or this could not see the warning.
const CASES: [Case; 2] = [
    Case {
        name: "as the app opens it",
        drop_env: &[],
        warns: false,
    },
    Case {
        name: "no terminal named",
        drop_env: &["TERM_PROGRAM", "TERM_PROGRAM_VERSION"],
        warns: true,
    },
];

/// The programs a listing of `OpenConsole.exe` processes names, each by where it
/// runs from: a line to each, without the blank ones.
fn hosts_of(listing: &str) -> Vec<String> {
    listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The folder a program is in, written to be compared: Windows' separators
/// whichever were written, no verbatim prefix (`\\?\`), no trailing separator,
/// and in lower case, as Windows reads a name.
fn folder_of(program: &str) -> String {
    let path = program.replace('/', "\\");
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
    let folder = path.rsplit_once('\\').map_or("", |(folder, _)| folder);
    folder.trim_end_matches('\\').to_lowercase()
}

/// Whether one of `hosts` runs from the folder `pane_host` is in: the console
/// host the pane host ships.
fn beside(hosts: &[String], pane_host: &str) -> bool {
    let ours = folder_of(pane_host);
    hosts.iter().any(|host| folder_of(host) == ours)
}

/// Whether a window did what its case says: it drew, it warned exactly where
/// the case says it must, and its console host is the shipped one.
fn holds(case: &Case, drawn: bool, warned: bool, ours: bool) -> bool {
    drawn && warned == case.warns && ours
}

/// What was seen of a window: whether it drew and warned, whose console host it
/// has (and, where it is not ours, whether the build has put one beside the pane
/// host at all), and the last of its screen.
fn detail(drawn: bool, warned: bool, ours: bool, shipped: bool, screen: &str) -> String {
    let said = if !drawn {
        "did not draw"
    } else if warned {
        "warns of conhost"
    } else {
        "no warning"
    };
    let host = if ours {
        "the shipped OpenConsole"
    } else if shipped {
        "not the shipped OpenConsole (Windows' own conhost?)"
    } else {
        "no OpenConsole.exe beside the pane host (cargo xtask conpty --into its folder)"
    };
    format!("{said}; {host}; its screen: {screen}")
}

/// What runs where the console host is: Windows.
#[cfg(windows)]
mod on_windows {
    use std::path::Path;

    use cf_e2e::live::report::{self, Check};
    use cf_e2e::live::window::{Open, READY};
    use cf_e2e::live::{self, ask, last_lines, Harness, Installed, Log};
    use cf_e2e::process::Run;
    use cf_e2e::rig::Rig;
    use cf_e2e::{cf, Error, Result};

    use super::{beside, detail, holds, hosts_of, Case, CASES, WARNING};

    /// The stand-in for Claude, which the rig puts first on the `PATH` of its
    /// windows: no window of this test runs it (Devin is opened on its own path),
    /// but a rig has one.
    const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

    /// The PowerShell that lists where the `OpenConsole.exe` processes running now
    /// run from, a line to each.
    const CONSOLE_HOSTS: &str = r#"Get-CimInstance Win32_Process -Filter "Name='OpenConsole.exe'" | ForEach-Object { $_.ExecutablePath }"#;

    /// The OpenConsole.exe processes running now, by where each runs from.
    fn console_hosts() -> Result<Vec<String>> {
        let ran = Run::new(live::command("powershell")?)
            .args(["-NoProfile", "-Command", CONSOLE_HOSTS])
            .inheriting_env()
            .run()?;
        if ran.code != Some(0) {
            return Err(Error::Daemon(format!(
                "PowerShell could not list the console hosts: {ran}"
            )));
        }
        Ok(hosts_of(&ran.stdout))
    }

    /// Opens Devin's window as `case` says, and says what it came to while it is open.
    fn check_of(
        rig: &Rig,
        install: &Installed,
        (index, case): (usize, &Case),
        pane_host: &str,
        log: &Log,
    ) -> Result<Check> {
        let open = Open::new(rig, install, "terminal", &format!("terminal-{index}"));
        let window = case
            .drop_env
            .iter()
            .fold(open, |open, name| open.without(name))
            .open(log)?;
        let drawn = window.still(READY);
        let screen = window.screen();
        let warned = screen.contains(WARNING);
        // Its console host, while it is open: the one beside the pane host.
        let ours = beside(&console_hosts()?, pane_host);
        let shipped = Path::new(pane_host)
            .with_file_name("OpenConsole.exe")
            .is_file();
        Ok(Check::new(
            install.harness().name(),
            case.name,
            holds(case, drawn, warned, ours),
            detail(drawn, warned, ours, shipped, &last_lines(&screen)),
        ))
    }

    /// Opens each case's window of Devin and judges what each shows of its terminal.
    pub fn run() -> Result {
        let log = Log::begin("live terminal")?;
        // What a window is opened on is the harness code's to say: built before anything is spent.
        ask::prepare()?;
        let pane_host = cf::pane_host()?.to_string_lossy().into_owned();
        let devin = log.found(Harness::Devin)?;
        log.say(format!("the pane host is {pane_host}"));
        let rig = Rig::start(live::rig(std::slice::from_ref(&devin), FAKE_AGENT))?;
        let mut checks = Vec::new();
        for (index, case) in CASES.iter().enumerate() {
            log.say(format!("window {}: {}", index + 1, case.name));
            checks.push(check_of(&rig, &devin, (index, case), &pane_host, &log)?);
        }
        rig.close()?;
        report::conclude(&log, "what each window came to", &checks)
    }
}

#[cfg(windows)]
#[test]
#[ignore = "opt-in, on Windows: opens two windows of Devin and sends nothing; cargo xtask live terminal (npm run live:terminal)"]
fn the_app_names_its_terminal_to_devin_and_the_console_host_under_each_window_is_the_shipped_one(
) -> Result {
    on_windows::run()
}

/// On a machine that is not Windows the check cannot look at what it is about, and
/// fails and says so.
#[cfg(not(windows))]
#[test]
#[ignore = "opt-in, on Windows: opens two windows of Devin and sends nothing; cargo xtask live terminal (npm run live:terminal)"]
fn the_app_names_its_terminal_to_devin_and_the_console_host_under_each_window_is_the_shipped_one(
) -> Result {
    Err(cf_e2e::Error::Daemon(
        "this check is for Windows, where the console host is: run it there, \
         `npm run windows -- --host <ssh host> --build -- cargo xtask live terminal`"
            .to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    #[test]
    fn devin_is_opened_twice_once_as_the_app_does_and_once_with_its_terminal_unnamed() {
        let [app, unnamed] = &CASES;
        assert_eq!(
            (app.name, app.drop_env, app.warns),
            ("as the app opens it", &[][..], false)
        );
        // The pane host names its terminal in these two variables, which Devin reads.
        assert_eq!(
            (unnamed.name, unnamed.drop_env, unnamed.warns),
            (
                "no terminal named",
                &["TERM_PROGRAM", "TERM_PROGRAM_VERSION"][..],
                true
            )
        );
        assert_eq!(
            WARNING,
            "Windows Console Host (conhost) has limited support"
        );
    }

    #[test]
    fn a_listing_of_the_console_hosts_is_a_line_to_each_without_the_blank_ones_or_the_returns() {
        assert_eq!(
            hosts_of("C:\\a\\OpenConsole.exe\r\n\r\n  D:\\b\\OpenConsole.exe  \r\n"),
            ["C:\\a\\OpenConsole.exe", "D:\\b\\OpenConsole.exe"]
        );
        assert!(hosts_of("").is_empty());
        assert!(hosts_of(" \r\n\n").is_empty());
    }

    #[test]
    fn a_console_host_is_the_shipped_one_when_it_runs_from_the_folder_the_pane_host_is_in() {
        let release = r"C:\Users\ngvoi\consensflow-build\app\src-tauri\target\release";
        let pane_host = format!(r"{release}\consensflow-bridge.exe");
        let ours = format!(r"{release}\OpenConsole.exe");
        assert!(beside(&hosts(&[&ours]), &pane_host));
        // Windows reads a name without regard to its case, and takes either separator.
        assert!(beside(
            &hosts(&[
                r"c:\USERS\ngvoi\ConsensFlow-Build\app\src-tauri\target\release\openconsole.exe"
            ]),
            &pane_host
        ));
        assert!(beside(
            &hosts(&[&ours]),
            "C:/Users/ngvoi/consensflow-build/app/src-tauri/target/release/consensflow-bridge.exe"
        ));
        // A path in its verbatim form is the same folder.
        assert!(beside(&hosts(&[&ours]), &format!(r"\\?\{pane_host}")));
        // Any one of several is enough, and the system's own conhost, or none, is not.
        assert!(beside(
            &hosts(&[r"C:\Windows\System32\OpenConsole.exe", &ours]),
            &pane_host
        ));
        assert!(!beside(
            &hosts(&[r"C:\Windows\System32\OpenConsole.exe"]),
            &pane_host
        ));
        assert!(!beside(&[], &pane_host));
        // A folder that only begins like the pane host's is another folder.
        assert!(!beside(
            &hosts(&[&format!(r"{release}2\OpenConsole.exe")]),
            &pane_host
        ));
    }

    #[test]
    fn a_program_with_no_folder_has_none_and_a_trailing_separator_is_not_part_of_one() {
        assert_eq!(folder_of("OpenConsole.exe"), "");
        assert_eq!(folder_of(r"C:\a\b\"), r"c:\a\b");
        assert_eq!(folder_of(r"C:\a\b\x.exe"), r"c:\a\b");
        assert_eq!(folder_of("/a/b/x.exe"), r"\a\b");
    }

    #[test]
    fn a_window_holds_when_it_drew_warned_only_where_its_case_says_and_has_the_shipped_console_host(
    ) {
        let [app, unnamed] = &CASES;
        // As the app opens it: no warning.
        assert!(holds(app, true, false, true));
        assert!(!holds(app, true, true, true), "it warns of conhost");
        // With the terminal unnamed: the warning is what shows the check can see it.
        assert!(holds(unnamed, true, true, true));
        assert!(
            !holds(unnamed, true, false, true),
            "it does not warn, so nothing was seen"
        );
        // Whichever, a window that drew nothing or runs on Windows' own console host fails.
        for case in &CASES {
            assert!(!holds(case, false, case.warns, true));
            assert!(!holds(case, true, case.warns, false));
        }
    }

    #[test]
    fn what_was_seen_of_a_window_says_whether_it_drew_warned_and_whose_console_host_it_has() {
        assert_eq!(
            detail(true, false, true, true, "a ⏎ b"),
            "no warning; the shipped OpenConsole; its screen: a ⏎ b"
        );
        assert_eq!(
            detail(true, true, false, true, "x"),
            "warns of conhost; not the shipped OpenConsole (Windows' own conhost?); its screen: x"
        );
        assert_eq!(
            detail(false, false, false, true, ""),
            "did not draw; not the shipped OpenConsole (Windows' own conhost?); its screen: "
        );
        // A build that put no console host beside the pane host says so.
        assert_eq!(
            detail(true, false, false, false, "x"),
            "no warning; no OpenConsole.exe beside the pane host (cargo xtask conpty --into its folder); its screen: x"
        );
    }

    /// A window opened as Windows' console host opens Devin's (zeewin, 2026-10-12): a
    /// line break at once, and the prompt seconds after, on the first look at it being
    /// "drawn" the check read a screen of blanks and found no warning in it.
    #[cfg(unix)]
    #[test]
    fn a_window_that_prints_a_line_break_first_has_drawn_when_its_prompt_is_there_and_not_before(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        use std::time::{Duration, Instant};

        use cf_e2e::live::window::Open;
        use cf_e2e::live::{Harness, Installed};
        use cf_e2e::rig::{Config, Rig};

        // Made before the rig starts, and long before a window runs it.
        let slow = tempfile::tempdir()?;
        let program = slow.path().join("slow-prompt");
        cf_e2e::files::write_executable(
            &program,
            "#!/bin/sh\nprintf '\\n'\nsleep 5\necho 'the prompt is drawn'\nsleep 120\n",
        )?;
        let rig = Rig::start(Config::new(env!("CARGO_BIN_EXE_fake-agent")))?;
        let home = rig.var("HOME").unwrap_or_default().to_owned();
        let install = Installed::at(Harness::Claude, &program, &home);
        let window = Open::new(&rig, &install, "terminal", "terminal-slow")
            .without_trust()
            .open(&cf_e2e::live::Log::new("live terminal test"))?;
        let opened = Instant::now();
        assert!(window.still(Duration::from_secs(60)));
        let screen = window.screen();
        assert!(screen.contains("the prompt is drawn"), "{screen:?}");
        // Three seconds of stillness after the prompt, which came five seconds in.
        assert!(
            opened.elapsed() >= Duration::from_secs(7),
            "{:?}",
            opened.elapsed()
        );
        drop(window);
        rig.close()?;
        Ok(())
    }
}
