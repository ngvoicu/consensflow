//! The job's reading of Git Bash's programs, on real ones: a script that Git Bash's `sh` runs
//! starts a `sleep` whose parent, as Windows names it, has gone (a forked copy of the shell),
//! and the job gives it the parent Git Bash says ([`repair_cygwin_parents`], [`ps_w`]); and an
//! ending, which looks at the job again and again, asks `ps -W` once for it (T-194, P3-2 and
//! P3-7). Git Bash is where Git for Windows puts it; a machine without it has nothing to try
//! this on, and the tests say so and end.
#![allow(clippy::disallowed_methods)] // A test's own programs.

use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use super::*;

/// No console window for a program the test starts.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Git Bash's `bash.exe`, where Git for Windows puts it.
fn git_bash() -> Option<PathBuf> {
    [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|bash| bash.is_file())
}

/// A job whose window runs `sh ./s.sh` in a folder of its own, the script running `sleep 30`:
/// Git Bash's `bash` is the window's program, put in the job a moment before it starts the
/// script (a child it starts in the first instant would escape the job).
struct Window {
    job: JobObject,
    program: Child,
    folder: PathBuf,
}

impl Window {
    /// A window in a folder of the system's temporary one named for the test (`label`) and
    /// this process.
    fn open(bash: &Path, label: &str) -> Self {
        let folder = std::env::temp_dir().join(format!("cf-panes-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("s.sh"), "#!/bin/sh\nsleep 30\n").unwrap();
        let program = Command::new(bash)
            .args(["-c", "sleep 2; sh ./s.sh"])
            .current_dir(&folder)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let job = JobObject::new().unwrap();
        job.assign(program.id()).unwrap();
        Self {
            job,
            program,
            folder,
        }
    }

    /// The job's members once one runs `sleep` for 30 s.
    fn until_the_sleep(&self) -> Vec<Member> {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let members = self.job.members().unwrap();
            if members.iter().any(is_the_sleep) {
                return members;
            }
            assert!(
                Instant::now() < until,
                "the script's sleep never began: {members:?}"
            );
            sleep(Duration::from_millis(250));
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let _ = self.program.kill();
        let _ = self.job.terminate();
        sleep(Duration::from_millis(500));
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn is_the_sleep(member: &Member) -> bool {
    member.program.eq_ignore_ascii_case("sleep.exe") && member.command.contains("30")
}

#[test]
fn the_sleep_of_a_script_has_the_script_for_its_parent_once_git_bash_has_said_so() {
    let Some(bash) = git_bash() else {
        eprintln!("no Git Bash here: nothing to try the repair on");
        return;
    };
    let mut window = Window::open(&bash, "repair");

    // As Windows names its parent, which has gone: the sleep has none among the job's members.
    window.job.ps = Arc::new(|_| None);
    let members = window.until_the_sleep();
    let sleep = members.iter().find(|one| is_the_sleep(one)).unwrap();
    assert_eq!(sleep.parent, None, "{members:?}");

    // With the parent Git Bash says: the script's shell, a member that runs `sh ./s.sh`.
    window.job.ps = Arc::new(ps_w);
    let members = window.job.members().unwrap();
    let sleep = members.iter().find(|one| is_the_sleep(one)).unwrap();
    let parent = members
        .iter()
        .find(|one| Some(one.key) == sleep.parent)
        .unwrap_or_else(|| panic!("the sleep has no parent among {members:?}"));
    assert!(
        parent.program.eq_ignore_ascii_case("sh.exe") && parent.command.contains("s.sh"),
        "{parent:?}"
    );
}

#[test]
fn a_look_again_asks_ps_for_what_it_does_not_hold_and_not_for_what_it_does() {
    let Some(bash) = git_bash() else {
        eprintln!("no Git Bash here: nothing to try the repair on");
        return;
    };
    let mut window = Window::open(&bash, "asks-once");
    window.job.ps = Arc::new(|_| None);
    window.until_the_sleep();

    // An ending's looks: the first holds the job, and `ps` is asked once, for the folder Git
    // Bash's programs are in; the looks after it hold nothing new, and ask nothing.
    let asked = Arc::new(AtomicUsize::new(0));
    let counting = Arc::clone(&asked);
    window.job.ps = Arc::new(move |folder| {
        counting.fetch_add(1, Ordering::SeqCst);
        ps_w(folder)
    });
    let mut held = HashMap::new();
    window.job.hold_into(&mut held).unwrap();
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    for _ in 0..3 {
        window.job.hold_into(&mut held).unwrap();
    }
    assert_eq!(asked.load(Ordering::SeqCst), 1, "a look again asked again");
    // And the one it held was mended: the members it makes of what it holds have the parent.
    let members = members_of(held.values());
    let sleep = members.iter().find(|one| is_the_sleep(one)).unwrap();
    assert!(sleep.parent.is_some(), "{members:?}");
}
