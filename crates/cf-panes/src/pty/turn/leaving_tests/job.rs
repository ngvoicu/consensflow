//! What the Windows tests of an ending read of a pane's job, told of the list of its
//! members, so that the reading is held where there is no Windows to run it. The job holds
//! the window's own program, which the tree starts at, and what it started, each with the
//! command line it was started with and the process of the job that started it where that
//! still runs.

use crate::background::Member;

/// Whether the job has a process whose command line holds `words` (in lower case, whatever
/// case the line has) that no process of the job started: a job left behind once the shell
/// that started it has ended is one. The shell is told to have gone by the job's parents,
/// never by what the command lines of the whole job say of it: the window's own program is
/// in the job as long as it runs, and a program started as `powershell -Command …` has
/// `-Command` in its own, whatever else has ended.
pub(super) fn on_its_own(job: &[Member], words: &str) -> bool {
    job.iter()
        .any(|member| member.parent.is_none() && member.command.to_lowercase().contains(words))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::ProcessKey;

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid,
            began: u64::from(pid),
        }
    }

    /// A process of the job, started by `parent` where that is a process of the job.
    fn member(pid: u32, command: &str, parent: Option<u32>) -> Member {
        Member {
            key: key(pid),
            command: command.to_owned(),
            program: String::new(),
            exe: String::new(),
            group: None,
            parent: parent.map(key),
        }
    }

    /// The window's program as the tests of Windows start it (`conpty_test::powershell`):
    /// with `-Command` and the whole script, which says what the agent's command runs.
    const PROGRAM: &str = r#""C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -Command "Write-Output 'READY#'; Start-Process -WindowStyle Hidden -FilePath 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' -ArgumentList '-NoProfile','-Command','Start-Process -WindowStyle Hidden -FilePath ''C:\Windows\System32\PING.EXE'' -ArgumentList ''-n'',''1004'',''127.0.0.1'''; Write-Output 'STARTED#'; Start-Sleep 1000""#;

    /// The shell the agent's command runs in, which starts the job and ends.
    const SHELL: &str = r"powershell.exe -NoProfile -Command Start-Process -WindowStyle Hidden -FilePath 'C:\Windows\System32\PING.EXE' -ArgumentList '-n','1004','127.0.0.1'";

    /// The job it leaves.
    const PING: &str = r#""C:\Windows\System32\PING.EXE" -n 1004 127.0.0.1"#;

    #[test]
    fn a_job_is_on_its_own_once_its_shell_has_gone_whatever_the_windows_own_program_says() {
        let program = member(10, PROGRAM, None);

        // The window's program alone has the shell's `-Command` among its arguments, as
        // long as it runs: a check that the job has none can never be met.
        assert!(PROGRAM.to_lowercase().contains("-command"));
        assert!(!on_its_own(std::slice::from_ref(&program), "-n 1004"));

        // The shell runs, and so does the job it started.
        let shell = member(20, SHELL, Some(10));
        assert!(!on_its_own(&[program.clone(), shell.clone()], "-n 1004"));
        let job = member(30, PING, Some(20));
        assert!(!on_its_own(&[program.clone(), shell, job], "-n 1004"));

        // The shell has ended: the job is in the job still, and no process of it started it.
        let job = member(30, PING, None);
        assert!(on_its_own(&[program, job], "-n 1004"));
    }
}
