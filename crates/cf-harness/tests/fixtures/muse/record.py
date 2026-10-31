#!/usr/bin/env python3
"""Records the Muse sessions the reader's fixtures are made of (T-703).

Runs the installed Muse binary directly (never the launcher, so no update check),
on its built-in fake model (`--provider echo`: offline, no login, no quota), in a
pty, in a throwaway home with its own XDG config/data/cache folders: nothing of
the owner's Muse (`~/.config/muse`, `~/.local/share/muse`) or of the owner's home
(Muse reads `~/.claude`, `~/.codex` and `~/.agents`) is read or written. Each case waits on
the session's own `session.jsonl`, not on the screen.

    python3 -I record.py <muse-bin> <out folder> [case...]

Cases: plain (two turns, then two Ctrl-Cs: a clean exit), retracted (Escape
before any output, the message put back, then a turn that completes, then a clean
exit), cut (SIGKILL with a turn at work), at-rest-killed (SIGKILL at rest after a
completed turn). Each leaves `<case>/session.jsonl`, `<case>/session-index.db`
(a consistent copy of the index, taken once the process is gone) and
`<case>/screen.txt` (the last bytes the pty printed). `cargo xtask
scrub-muse-session` makes the fixtures of `crates/cf-harness/tests/fixtures/muse`
from them. It is a script (the rig's real-binary cases are MUS-12's): run it as
`python3 -I crates/cf-harness/tests/fixtures/muse/record.py <muse-bin> <out folder> [case...]`.
"""
import json, os, pty, select, shutil, signal, sqlite3, subprocess, sys, tempfile, time

CASES = ['plain', 'retracted', 'cut', 'at-rest-killed', 'hooks', 'cut-resumed', 'quit-mid-turn',
         'steered', 'index']

# A hook on SessionStart whose stdout adds to the model's context, and one on
# UserPromptSubmit that adds nothing, given for the run alone (`-c hooks=`).
HOOKS = json.dumps({
    'SessionStart': [{'matcher': '', 'hooks': [{'type': 'command', 'command': "printf 'Role text from a SessionStart hook.'"}]}],
    'UserPromptSubmit': [{'matcher': '', 'hooks': [{'type': 'command', 'command': 'true'}]}],
})
ARGS = {'hooks': ('-c', 'hooks=' + HOOKS)}


class Window:
    def __init__(self, bin, home, work, delay_ms, args=()):
        env = {k: v for k, v in os.environ.items() if not k.startswith('MUSE_')}
        # The home too: Muse reads the user's skills and instruction files from
        # `~/.claude`, `~/.codex` and `~/.agents` into the model's request, and the
        # echo provider needs no login (no keychain).
        env.update(HOME=home, XDG_CONFIG_HOME=home + '/config', XDG_DATA_HOME=home + '/data',
                   XDG_CACHE_HOME=home + '/cache', MUSE_NO_AUTO_UPDATE='1',
                   TERM='xterm-256color', COLUMNS='110', LINES='40')
        self.bin, self.home, self.work, self.delay_ms = bin, home, work, delay_ms
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.chdir(work)
            os.execve(bin, [bin, '--provider', 'echo', '--echo-delay-ms', str(delay_ms),
                            '--trust-workspace', *args], env)
        import fcntl, termios, struct
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 110, 0, 0))
        self.screen = b''
        self.data = home + '/data/muse'

    def pump(self, seconds):
        end = time.time() + seconds
        while time.time() < end:
            ready, _, _ = select.select([self.fd], [], [], 0.05)
            if ready:
                try:
                    chunk = os.read(self.fd, 65536)
                except OSError:
                    return
                if not chunk:
                    return
                self.screen = (self.screen + chunk)[-20000:]

    def send(self, data):
        os.write(self.fd, data)

    def paste(self, text):
        self.send(b'\x1b[200~' + text.encode() + b'\x1b[201~')

    def log(self):
        found = []
        for root, _, files in os.walk(self.data + '/sessions'):
            if 'session.jsonl' in files and '/subagent/' not in root + '/':
                found.append(root + '/session.jsonl')
        # The session the window shows is the one written last.
        return max(found, key=os.path.getmtime) if found else None

    def records(self):
        path = self.log()
        if not path:
            return []
        out = []
        with open(path) as f:
            for line in f:
                try:
                    out.append(json.loads(line))
                except ValueError:
                    pass
        return out

    def until(self, what, test, limit=60):
        end = time.time() + limit
        while time.time() < end:
            self.pump(0.2)
            if test(self.records()):
                return
        raise SystemExit(f'timed out waiting for {what}')

    def alive(self):
        try:
            return os.waitpid(self.pid, os.WNOHANG) == (0, 0)
        except ChildProcessError:
            return False

    def wait_gone(self, limit=15):
        end = time.time() + limit
        while time.time() < end:
            self.pump(0.2)
            pid, _ = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                return
        raise SystemExit('the window did not end')


def kinds(records, kind):
    return [r for r in records if (r.get('payload') or {}).get('kind') == kind
            or ((r.get('payload') or {}).get('event') or {}).get('kind') == kind]


def run_event(records, kind):
    return [r for r in records if ((r.get('payload') or {}).get('event') or {}).get('kind') == kind
            or (r.get('payload') or {}).get('kind') == kind]


def terminals(records):
    return [r for r in records if 'terminal' in json.dumps(r.get('payload', {}))[:200]
            and run_event(records, 'terminal').__contains__(r)]


def count(records, kind):
    return len(run_event(records, kind))


def prompt_ready(w):
    w.pump(4)  # the TUI's start; no trust dialog with --trust-workspace


def case_plain(w):
    prompt_ready(w)
    w.paste('Say hello.'); w.pump(0.3); w.send(b'\r')
    w.until('first terminal', lambda r: count(r, 'terminal') >= 1)
    w.pump(2)
    w.paste('Say goodbye.'); w.pump(0.3); w.send(b'\r')
    w.until('second terminal', lambda r: count(r, 'terminal') >= 2)
    w.pump(2)
    w.send(b'\x03'); w.pump(1); w.send(b'\x03')
    w.wait_gone()


def case_retracted(w):
    prompt_ready(w)
    w.paste('Stop me before a word.'); w.pump(0.3); w.send(b'\r')
    w.until('the run started', lambda r: count(r, 'started') >= 1)
    w.pump(1.5)
    w.send(b'\x1b')
    w.until('the retract', lambda r: count(r, 'run_retracted') >= 1)
    w.pump(1.5)
    w.send(b'\x03')  # clear the message put back in the box
    w.pump(0.5)
    w.paste('Now answer.'); w.pump(0.3); w.send(b'\r')
    w.until('second terminal', lambda r: count(r, 'terminal') >= 2)
    w.pump(2)
    w.send(b'\x03'); w.pump(1); w.send(b'\x03')
    w.wait_gone()


def case_cut(w):
    prompt_ready(w)
    w.paste('Say hello.'); w.pump(0.3); w.send(b'\r')
    w.until('first terminal', lambda r: count(r, 'terminal') >= 1)
    w.pump(2)
    w.paste('Be cut.'); w.pump(0.3); w.send(b'\r')
    w.until('the second run started', lambda r: count(r, 'started') >= 2)
    w.pump(2)
    os.kill(w.pid, signal.SIGKILL)
    w.wait_gone()


def case_at_rest_killed(w):
    prompt_ready(w)
    w.paste('Say hello.'); w.pump(0.3); w.send(b'\r')
    w.until('first terminal', lambda r: count(r, 'terminal') >= 1)
    w.pump(3)
    os.kill(w.pid, signal.SIGKILL)
    w.wait_gone()


def throwaway():
    """A throwaway Muse home (XDG config, data, cache) and a git workspace in it."""
    home = tempfile.mkdtemp(prefix='muse-rec-')
    os.makedirs(home + '/config/muse')
    with open(home + '/config/muse/settings.json', 'w') as f:
        f.write('{"schema_version":1}')
    work = home + '/work'
    os.makedirs(work)
    subprocess.run(['git', 'init', '-q', work], check=True)
    return home, work


def case_hooks(w):
    case_plain(w)


def case_cut_resumed(w):
    case_cut(w)
    w.__init__(w.bin, w.home, w.work, w.delay_ms, ('resume', '--last'))
    w.until('the orphaned run reconciled', lambda r: any(
        (e.get('payload') or {}).get('kind') == 'session_resumed' for e in r))
    w.pump(3)
    w.send(b'\x03'); w.pump(1); w.send(b'\x03')
    w.wait_gone()


def case_quit_mid_turn(w):
    prompt_ready(w)
    w.paste('Say hello.'); w.pump(0.3); w.send(b'\r')
    w.until('first terminal', lambda r: count(r, 'terminal') >= 1)
    w.pump(2)
    w.paste('Be quit.'); w.pump(0.3); w.send(b'\r')
    w.until('the second run started', lambda r: count(r, 'started') >= 2)
    w.pump(2)
    # Muse 1.4.13: the first Ctrl-C interrupts the turn (its message back in the
    # box), the second clears the box, the next two quit.
    for _ in range(4):
        w.send(b'\x03'); w.pump(1.2)
    w.wait_gone()


def case_steered(w):
    prompt_ready(w)
    w.paste('Say hello.'); w.pump(0.3); w.send(b'\r')
    w.until('the run started', lambda r: count(r, 'started') >= 1)
    w.pump(1.5)
    w.paste('And then this.'); w.pump(0.3); w.send(b'\r')
    w.until('both answered', lambda r: count(r, 'terminal') >= 2, 90)
    w.pump(2)
    w.send(b'\x03'); w.pump(1); w.send(b'\x03')
    w.wait_gone()


def index_rows(home):
    """The index's rows as Muse left them: session id, prompt count, whether its
    latest segment ended."""
    path = home + '/data/muse/session-index.db'
    if not os.path.exists(path):
        return []
    db = sqlite3.connect('file:' + path + '?mode=ro', uri=True)
    try:
        return [list(row) for row in db.execute(
            'SELECT session_id, prompt_count, latest_segment_terminated FROM sessions ORDER BY session_id')]
    finally:
        db.close()


def case_index(w):
    """What the index says of three sessions, as each ends and as the next starts:
    `index-states.json`, a step's name and the rows after it."""
    states = []
    step = lambda name: states.append({'step': name, 'rows': index_rows(w.home),
                                       'logs': sorted(logs(w.data))})
    turn = lambda text: (w.paste(text), w.pump(0.3), w.send(b'\r'),
                         w.until(text, lambda r: sum(1 for e in r if e.get('payload', {}).get('event', {}).get('kind') == 'terminal') >= 1))
    quit = lambda: (w.send(b'\x03'), w.pump(1), w.send(b'\x03'), w.wait_gone())
    prompt_ready(w); turn('One.'); w.pump(5); step('first session answered, open')
    quit(); step('first session quit cleanly')
    w.__init__(w.bin, w.home, w.work, w.delay_ms); prompt_ready(w); step('second session up')
    turn('Two.'); w.pump(5); step('second session answered, open')
    quit(); step('second session quit cleanly')
    w.__init__(w.bin, w.home, w.work, w.delay_ms); prompt_ready(w)
    turn('Three.'); w.pump(3)
    os.kill(w.pid, signal.SIGKILL); w.wait_gone(); step('third session killed at rest')
    w.__init__(w.bin, w.home, w.work, w.delay_ms, ('resume', '--last')); w.pump(6)
    step('third session resumed')
    quit(); step('third session resumed and quit')
    w.__init__(w.bin, w.home, w.work, w.delay_ms); prompt_ready(w); step('a fourth window up')
    quit(); step('the fourth window quit')
    w.states = states


def logs(data):
    """The ids of the sessions whose logs are on disk."""
    found = []
    for root, _, files in os.walk(data + '/sessions'):
        if 'session.jsonl' in files and '/subagent/' not in root + '/':
            found.append(os.path.basename(root))
    return found


def main():
    bin, out = sys.argv[1], os.path.abspath(sys.argv[2])
    for case in sys.argv[3:] or CASES:
        home, work = throwaway()
        w = Window(bin, home, work, 6000, ARGS.get(case, ()))
        try:
            globals()['case_' + case.replace('-', '_')](w)
        finally:
            if w.alive():
                os.kill(w.pid, signal.SIGKILL)
        dest = os.path.join(out, case)
        os.makedirs(dest, exist_ok=True)
        if hasattr(w, 'states'):
            with open(dest + '/index-states.json', 'w') as f:
                json.dump(w.states, f, indent=1)
            print(case, 'recorded:', len(w.states), 'steps')
            continue
        shutil.copy(w.log(), dest + '/session.jsonl')
        src = sqlite3.connect('file:' + w.data + '/session-index.db?mode=ro', uri=True)
        copy = sqlite3.connect(dest + '/session-index.db')
        src.backup(copy); copy.close(); src.close()
        with open(dest + '/screen.txt', 'wb') as f:
            f.write(w.screen)
        with open(dest + '/home.txt', 'w') as f:
            f.write(home + '\n' + work + '\n')
        print(case, 'recorded:', len(w.records()), 'records')


if __name__ == '__main__':
    main()
