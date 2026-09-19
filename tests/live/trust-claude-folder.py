"""Trust one folder for Claude Code, once, the way a person would.

Claude asks whether to trust an unknown folder before an interactive session
starts, and nobody can answer that in an unattended pane. The live bench works
in one fixed folder; this answers Claude's own dialog there through a real
terminal, then exits. Usage: python3 trust-claude-folder.py <folder>
"""
import os, pty, re, select, signal, sys, time

folder = os.path.abspath(sys.argv[1])
H = os.environ["HOME"]
env = {"HOME": H, "USER": os.environ["USER"], "LOGNAME": os.environ["USER"], "LANG": "en_US.UTF-8",
       "TERM": "xterm-256color", "PATH": f"{H}/.local/bin:/usr/bin:/bin"}
pid, fd = pty.fork()
if pid == 0:
    os.chdir(folder)
    os.execve(f"{H}/.local/bin/claude", [f"{H}/.local/bin/claude", "--model", "sonnet"], env)
screen, answered, ready = b"", False, False
end = time.time() + 60
while time.time() < end and not ready:
    if select.select([fd], [], [], 0.25)[0]:
        try:
            screen += os.read(fd, 65536)
        except OSError:
            break
    text = re.sub(rb"\x1b\[[0-9;?]*[ -/]*[@-~]", b"", screen).decode("utf8", "replace")
    if not answered and re.search(r"trust (the files|this folder)", text, re.I):
        os.write(fd, b"\r")
        answered = True
        screen = b""
    elif re.search(r"(bypass permissions|\? for shortcuts|shortcuts)", text, re.I):
        ready = True
os.kill(pid, signal.SIGTERM)
drain_until = time.time() + 3
while time.time() < drain_until:
    if select.select([fd], [], [], 0.2)[0]:
        try:
            os.read(fd, 65536)
        except OSError:
            break
try:
    os.kill(pid, signal.SIGKILL)
except ProcessLookupError:
    pass
os.close(fd)
os.waitpid(pid, 0)
print("trusted" if answered else ("already trusted" if ready else "no prompt seen"))
