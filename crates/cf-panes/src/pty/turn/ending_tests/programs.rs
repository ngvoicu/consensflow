//! The programs the macOS tests of an ending run in a pane: each builds the tree of
//! a harness's window (a program that starts servers, and an agent that starts
//! commands in shells), says what it started once it has, and waits.

/// The window of a harness that keeps its servers in the program's group, the
/// way Claude Code does, once its turn is told to go: a server the window had
/// before the turn; a server started after it that launches a browser, which
/// leaves the group; and what the agent runs, a command each, a shell in a
/// group of its own with a job: one that handles the request to end (it writes
/// `RELEASED`), and, where `STUBBORN` is set, one that ignores the requests.
/// The program says what it has started, and their numbers, once each has.
pub(super) const TURN: &str = r#"
import json, os, subprocess, sys, time

BROWSER = "import os, time; os.setsid(); print(os.getpid(), flush=True); time.sleep(35)"
SERVER = (
    "import subprocess, sys, time; "
    "browser = subprocess.Popen([sys.executable, '-c', " + repr(BROWSER) + "], stdout=subprocess.PIPE, text=True); "
    "print(browser.stdout.readline().strip(), flush=True); time.sleep(36)"
)

def command(script):
    # A command of the agent's: a shell in a group of its own, which says its job's number.
    shell = subprocess.Popen(["/bin/sh", "-c", script], stdout=subprocess.PIPE, text=True,
                             preexec_fn=os.setpgrp)
    return shell, int(shell.stdout.readline())

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
server = subprocess.Popen([sys.executable, "-c", SERVER], stdout=subprocess.PIPE, text=True)
browser = int(server.stdout.readline())
handled, handled_job = command("trap 'echo x > \"$RELEASED\"; exit 0' TERM HUP; sleep 32 & echo $!; wait")
numbers = {"program": os.getpid(), "before": before.pid, "server": server.pid, "browser": browser,
           "handled": handled.pid, "handled_job": handled_job}
if os.environ.get("STUBBORN"):
    stubborn, stubborn_job = command("trap '' TERM HUP; sleep 37 & echo $!; wait")
    numbers.update(stubborn=stubborn.pid, stubborn_job=stubborn_job)
print("started " + json.dumps(numbers), flush=True)
time.sleep(60)
"#;

/// A command whose handler for the request to end starts a job of its own, and
/// ends: the job is begun while the command is given its grace, and its parent
/// is gone by the time it is found. The handler is for TERM, the first of the two requests
/// (TERM and then HUP, which follow each other within a moment), and HUP is ignored: a
/// handler for both runs once or twice by how close the two are, and starts a job each time.
pub(super) const SPAWNER: &str = r#"
import json, os, subprocess, time

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
shell = subprocess.Popen(
    ["/bin/sh", "-c", "trap 'sleep 38 & echo $! > \"$CHILD\"; exit 0' TERM; trap '' HUP; sleep 32 & echo $!; wait"],
    stdout=subprocess.PIPE, text=True, preexec_fn=os.setpgrp)
job = int(shell.stdout.readline())
print("started " + json.dumps({"program": os.getpid(), "before": before.pid, "shell": shell.pid, "job": job}), flush=True)
time.sleep(60)
"#;

/// More commands than a frame names: twenty, each a shell with a job.
pub(super) const MANY: &str = r#"
import json, os, subprocess, time

print("ready", flush=True)
input()
numbers = {"program": os.getpid()}
for n in range(20):
    shell = subprocess.Popen(["/bin/sh", "-c", "sleep 31 & echo $!; wait"],
                             stdout=subprocess.PIPE, text=True, preexec_fn=os.setpgrp)
    numbers["shell%d" % n] = shell.pid
    numbers["job%d" % n] = int(shell.stdout.readline())
print("started " + json.dumps(numbers), flush=True)
time.sleep(60)
"#;

/// A command of the turn's whose shell ignores the request to end, with a job, so
/// that an ending goes through the whole of its grace; and then, when the test types
/// into the window while the ending waits, new work of the window's own: another
/// command, a shell in a group of its own with a job, as a human's message or a new
/// turn starts it, which the old turn's mark takes for the turn's work all the same.
pub(super) const NEW: &str = r#"
import json, os, subprocess, sys, time

def command(script):
    shell = subprocess.Popen(["/bin/sh", "-c", script], stdout=subprocess.PIPE, text=True,
                             preexec_fn=os.setpgrp)
    return shell, int(shell.stdout.readline())

before = subprocess.Popen(["sleep", "33"])
print("ready", flush=True)
input()
old, old_job = command("trap '' TERM HUP; sleep 37 & echo $!; wait")
print("started " + json.dumps({"program": os.getpid(), "before": before.pid,
                               "old": old.pid, "old_job": old_job}), flush=True)
input()
new, new_job = command("sleep 35 & echo $!; wait")
print("more " + json.dumps({"new": new.pid, "new_job": new_job}), flush=True)
time.sleep(60)
"#;

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
