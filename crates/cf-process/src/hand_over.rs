//! A program handed something and let go: the system's opener, given a file
//! to open, which may hand it to an app and end at once (`open`, Explorer), or
//! stay as long as the app it started runs (an `xdg-open` that starts a browser
//! in its own place). It is waited for a moment, for what it says when it fails
//! at once; then it is let go, never ended: it leads a process group of its own
//! on Unix, so nothing sent to this one's reaches it, and on Windows it stays
//! out of the job that ends every child of this process with it.

use std::process::Stdio;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::file::error_code;

use crate::execute::hide_window;
use crate::group::lead;
use crate::Run;

/// What came of a program handed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handed {
    /// It ended within the moment it was waited for: the code it ended with,
    /// none where a signal ended it.
    Ended(Option<i32>),
    /// It runs on, let go.
    Running,
}

/// Starts `run` with the environment `env`, all of it and nothing inherited,
/// its streams let go, and waits for it `within` at most: what came of it, or
/// Node's words for why it did not start (`spawn <file> ENOENT`). One still
/// running then is let go, never ended, and waited for apart, so it leaves
/// nothing behind when it ends.
pub async fn hand_over(run: &Run, env: &Env, within: Duration) -> Result<Handed, String> {
    let mut command = tokio::process::Command::from(run.command());
    command
        .env_clear()
        .envs(env.iter())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_window(&mut command);
    lead(&mut command);
    let mut child = command.spawn().map_err(|failed| {
        format!(
            "spawn {} {}",
            run.program.to_string_lossy(),
            error_code(&failed)
        )
    })?;
    match tokio::time::timeout(within, child.wait()).await {
        Ok(Ok(status)) => Ok(Handed::Ended(status.code())),
        Ok(Err(failed)) => Err(format!(
            "{} could not be waited for: {failed}",
            run.program.to_string_lossy()
        )),
        Err(_) => {
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
            Ok(Handed::Running)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;

    /// A shell running `script`: `sh -c` on Unix, `cmd /d /c` on Windows.
    fn shell(script: &str) -> Run {
        #[cfg(unix)]
        let (program, flags) = (PathBuf::from("/bin/sh"), vec!["-c"]);
        #[cfg(windows)]
        let (program, flags) = (
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            vec!["/d", "/c"],
        );
        Run {
            program,
            args: flags
                .into_iter()
                .chain([script])
                .map(OsString::from)
                .collect(),
            verbatim: false,
        }
    }

    fn system_env() -> Env {
        #[cfg(unix)]
        return Env::from_vars([("PATH", "/usr/bin:/bin")]);
        #[cfg(windows)]
        return Env::from_vars([
            ("SystemRoot", r"C:\Windows"),
            ("PATH", r"C:\Windows\System32"),
        ]);
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn a_program_that_ends_at_once_says_its_code() {
        let runtime = runtime();
        let handed = |script| {
            runtime.block_on(hand_over(
                &shell(script),
                &system_env(),
                Duration::from_secs(10),
            ))
        };
        assert_eq!(handed("exit 0"), Ok(Handed::Ended(Some(0))));
        assert_eq!(handed("exit 3"), Ok(Handed::Ended(Some(3))));
    }

    /// The opener that stays while the browser it started runs is let go at
    /// the end of its moment, and goes on to its end: nothing ends it.
    #[cfg(unix)]
    #[test]
    fn a_program_still_running_is_let_go_and_runs_on_to_its_end() {
        let folder = tempfile::tempdir().unwrap();
        let mark = folder.path().join("ended");
        let script = format!("sleep 1; echo done > '{}'", mark.display());
        let started = std::time::Instant::now();
        let runtime = runtime();
        let handed = runtime.block_on(hand_over(
            &shell(&script),
            &system_env(),
            Duration::from_millis(100),
        ));
        assert_eq!(handed, Ok(Handed::Running));
        assert!(started.elapsed() < Duration::from_millis(900));
        drop(runtime);
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !mark.exists() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(std::fs::read_to_string(&mark).unwrap(), "done\n");
    }

    #[test]
    fn a_program_that_is_not_there_says_spawn_and_enoent() {
        let run = Run {
            program: PathBuf::from("/nonexistent/opener"),
            args: vec![OsString::from("file.html")],
            verbatim: false,
        };
        assert_eq!(
            runtime().block_on(hand_over(&run, &system_env(), Duration::from_secs(1))),
            Err("spawn /nonexistent/opener ENOENT".to_owned())
        );
    }
}
