#!/usr/bin/env python3
"""A pty driver with a terminal emulator (pyte) for T-445's probes.

Spawns a harness TUI in a pty, feeds its output into a pyte screen (with
history), answers the common terminal queries, and writes plain-text snapshots
(and, on request, colour runs). One session at a time; a hard alarm limit; the
whole process group is killed at the end.

T-445's driver (.runs/T-model-switch), kept here for the chief-harness probes (CHF-02).
"""
import os, pty, re, select, signal, struct, subprocess, sys, time, fcntl, termios, json
import pyte

KEYS = {
    "enter": b"\r", "esc": b"\x1b", "tab": b"\t", "btab": b"\x1b[Z",
    "up": b"\x1b[A", "down": b"\x1b[B", "right": b"\x1b[C", "left": b"\x1b[D",
    "ctrl-c": b"\x03", "ctrl-d": b"\x04", "ctrl-l": b"\x0c", "ctrl-p": b"\x10",
    "ctrl-x": b"\x18", "ctrl-t": b"\x14", "ctrl-o": b"\x0f", "ctrl-n": b"\x0e",
    "ctrl-a": b"\x01", "ctrl-e": b"\x05", "ctrl-u": b"\x15", "ctrl-k": b"\x0b",
    "ctrl-r": b"\x12", "ctrl-g": b"\x07", "ctrl-b": b"\x02", "ctrl-s": b"\x13",
    "shift-tab": b"\x1b[Z", "alt-p": b"\x1bp", "pgup": b"\x1b[5~", "pgdn": b"\x1b[6~",
    "home": b"\x1b[H", "end": b"\x1b[F", "bs": b"\x7f",
}


class Session:
    def __init__(self, argv, env, cwd, cols=150, rows=50, limit=1500, rawlog=None, history=4000, answer=True):
        self.answer = answer
        self.cols, self.rows = cols, rows
        self.screen = pyte.HistoryScreen(cols, rows, history=history, ratio=0.5)
        self.stream = pyte.ByteStream(self.screen)
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        self.proc = subprocess.Popen(
            argv, stdin=slave, stdout=slave, stderr=slave, cwd=cwd, env=env,
            preexec_fn=os.setsid, close_fds=True,
        )
        os.close(slave)
        self.last_change = time.time()
        self.rawlog = open(rawlog, "wb") if rawlog else None
        self.limit_at = time.time() + limit
        self.tail = b""

    # --- reading -------------------------------------------------------
    def _answer_queries(self, data):
        if not self.answer:
            return
        buf = self.tail + data
        self.tail = buf[-16:]
        out = b""
        if b"\x1b[6n" in buf:
            out += b"\x1b[%d;%dR" % (self.screen.cursor.y + 1, self.screen.cursor.x + 1)
        if b"\x1b[c" in buf or b"\x1b[0c" in buf:
            out += b"\x1b[?62;c"
        if b"\x1b[>c" in buf or b"\x1b[>0c" in buf:
            out += b"\x1b[>0;0;0c"
        if b"\x1b]10;?" in buf:
            out += b"\x1b]10;rgb:cccc/cccc/cccc\x1b\\"
        if b"\x1b]11;?" in buf:
            out += b"\x1b]11;rgb:0000/0000/0000\x1b\\"
        if out:
            try:
                os.write(self.master, out)
            except OSError:
                pass

    def pump(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            if time.time() > self.limit_at:
                raise TimeoutError("hard limit")
            r, _, _ = select.select([self.master], [], [], min(0.2, max(0.0, end - time.time())))
            if self.master in r:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    return False
                if not data:
                    return False
                if self.rawlog:
                    self.rawlog.write(data)
                    self.rawlog.flush()
                self._answer_queries(data)
                self.stream.feed(data)
                self.last_change = time.time()
        return True

    def alive(self):
        return self.proc.poll() is None

    def wait_quiet(self, quiet=4.0, maxwait=90.0):
        """Pump until no output for `quiet` seconds (or maxwait)."""
        start = time.time()
        self.last_change = time.time()
        while time.time() - start < maxwait:
            self.pump(0.5)
            if time.time() - self.last_change >= quiet:
                return True
            if not self.alive():
                return False
        return False

    def text(self, with_history=False):
        lines = []
        if with_history:
            for row in self.screen.history.top:
                lines.append(self._row_text(row))
        for y in range(self.rows):
            lines.append("".join(self.screen.buffer[y][x].data for x in range(self.cols)).rstrip())
        return lines

    def _row_text(self, row):
        return "".join(row[x].data if x in row else " " for x in range(self.cols)).rstrip()

    def wait_for(self, pattern, maxwait=90.0, history=True):
        rx = re.compile(pattern)
        start = time.time()
        while time.time() - start < maxwait:
            self.pump(0.5)
            if rx.search("\n".join(self.text(history))):
                return True
            if not self.alive():
                return False
        return False

    # --- writing -------------------------------------------------------
    def send(self, data):
        if isinstance(data, str):
            data = data.encode()
        os.write(self.master, data)

    def key(self, name, settle=0.4):
        self.send(KEYS[name])
        self.pump(settle)

    def type(self, text, enter=True, settle=0.8):
        self.send(text)
        self.pump(settle)
        if enter:
            self.send(b"\r")
            self.pump(0.3)

    # --- snapshots -----------------------------------------------------
    def snap(self, path, title="", with_history=True, colors=False):
        lines = self.text(with_history)
        while lines and not lines[-1].strip():
            lines.pop()
        with open(path, "w") as f:
            if title:
                f.write(f"# {title}\n# screen {self.cols}x{self.rows}, history lines above the screen included: {with_history}\n")
            f.write("\n".join(lines) + "\n")
        if colors:
            self.snap_colors(path + ".colors", with_history)

    def snap_colors(self, path, with_history=True):
        """Runs of equal attributes: [fg bg flags]text, one screen row per line."""
        def attrs(ch):
            f = []
            if ch.bold: f.append("bold")
            if ch.italics: f.append("italic")
            if ch.underscore: f.append("underline")
            if ch.strikethrough: f.append("strike")
            if ch.reverse: f.append("reverse")
            return (ch.fg, ch.bg, ",".join(f))

        def runs(row_get):
            out, cur, buf = [], None, ""
            for x in range(self.cols):
                ch = row_get(x)
                a = attrs(ch)
                if a != cur:
                    if buf.strip() or (cur and cur[1] != "default"):
                        out.append((cur, buf))
                    cur, buf = a, ""
                buf += ch.data
            if buf.strip() or (cur and cur[1] != "default"):
                out.append((cur, buf))
            return out

        blank = pyte.screens.Char(" ")
        rows = []
        if with_history:
            for row in self.screen.history.top:
                rows.append(lambda x, row=row: row[x] if x in row else blank)
        for y in range(self.rows):
            rows.append(lambda x, y=y: self.screen.buffer[y][x])
        lines = []
        for get in rows:
            rs = runs(get)
            if not rs:
                lines.append("")
                continue
            lines.append(" | ".join(f"[fg={a[0]} bg={a[1]}{' ' + a[2] if a[2] else ''}]{t.rstrip()!r}" for a, t in rs if a))
        while lines and not lines[-1].strip():
            lines.pop()
        with open(path, "w") as f:
            f.write("\n".join(lines) + "\n")

    # --- ending --------------------------------------------------------
    def kill(self):
        if self.alive():
            try:
                os.killpg(self.proc.pid, signal.SIGTERM)
            except (ProcessLookupError, PermissionError):
                pass
            for _ in range(20):
                if not self.alive():
                    break
                time.sleep(0.25)
            if self.alive():
                try:
                    os.killpg(self.proc.pid, signal.SIGKILL)
                except (ProcessLookupError, PermissionError):
                    pass
        try:
            os.close(self.master)
        except OSError:
            pass
        if self.rawlog:
            self.rawlog.close()


def clean_env(extra, home):
    """A clean environment: no CLAUDE_CODE_*, no keys that switch billing."""
    env = {
        "HOME": home, "USER": os.environ.get("USER", "gabrielvoicu"),
        "LOGNAME": os.environ.get("USER", "gabrielvoicu"),
        "PATH": "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:" + os.path.expanduser("~/.local/bin"),
        "TERM": "xterm-256color", "COLORTERM": "truecolor", "LANG": "en_US.UTF-8", "SHELL": "/bin/zsh",
        "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
    }
    env.update(extra)
    return env
