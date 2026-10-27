//! The programs the macOS tests of what a turn leaves run in a pane: each builds the tree of
//! a harness's window (a program that starts servers, and an agent that starts
//! commands in shells), says what it started once it has, and waits.

/// A job the agent's command left in a session of its own: its shell and the
/// program that started it are gone, and `sleep` is a system binary, which shows
/// no environment to anyone.
pub(super) const LEFT: &str = r#"
import json, os, subprocess, time

LEAVER = (
    "import subprocess; print(subprocess.Popen(['sleep', '39'], start_new_session=True, "
    "stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).pid)"
)
print("ready", flush=True)
input()
shell = subprocess.run(["/bin/sh", "-c", '/usr/bin/python3 -c "$LEAVER"'], capture_output=True,
                       text=True, env={**os.environ, "LEAVER": LEAVER})
job = int(shell.stdout.strip())
print("started " + json.dumps({"program": os.getpid(), "job": job}), flush=True)
time.sleep(60)
"#;

/// A window of a harness named `codex`, whose binary and the helper it keeps for
/// the session (a program named for it beside it, which runs a shell with a job)
/// are in `BINS`, a folder with a space in its name. After the mark the helper
/// runs its shell, the window starts a server it did not need before (no shell: a
/// lazy MCP server, with its browser), and the agent runs a command.
pub(super) const HELPERS: &str = r#"
import json, os, subprocess, sys, time

BROWSER = "import os, time; os.setsid(); print(os.getpid(), flush=True); time.sleep(35)"
SERVER = (
    "import subprocess, sys, time; "
    "browser = subprocess.Popen([sys.executable, '-c', " + repr(BROWSER) + "], stdout=subprocess.PIPE, text=True); "
    "print(browser.stdout.readline().strip(), flush=True); time.sleep(36)"
)
bins = os.environ["BINS"]

before = subprocess.Popen([bins + "/codex"])
print("ready", flush=True)
input()
helper = subprocess.Popen([bins + "/codex-code-mode-host", "echo $$; sleep 33 & echo $!; wait"],
                          stdout=subprocess.PIPE, text=True)
helper_shell = int(helper.stdout.readline())
helper_job = int(helper.stdout.readline())
server = subprocess.Popen([sys.executable, "-c", SERVER], stdout=subprocess.PIPE, text=True)
browser = int(server.stdout.readline())
tool = subprocess.Popen(["/bin/sh", "-c", "sleep 32 & echo $!; wait"],
                        stdout=subprocess.PIPE, text=True, preexec_fn=os.setpgrp)
tool_job = int(tool.stdout.readline())
print("started " + json.dumps({
    "program": os.getpid(), "before": before.pid, "helper": helper.pid, "helper_shell": helper_shell,
    "helper_job": helper_job, "server": server.pid, "browser": browser, "tool": tool.pid,
    "tool_job": tool_job,
}), flush=True)
time.sleep(60)
"#;
