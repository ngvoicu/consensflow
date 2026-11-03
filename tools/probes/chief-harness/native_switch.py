#!/usr/bin/env python3 -I
"""The human's own model switch in a chief's window, live (CHF-10): the harness's window is
opened on model A, answers one turn, is switched by the human's `/model` with the key that
keeps the switch to the conversation, answers one turn on model B, and is quit. Then:

  4. ConsensFlow's own reader of that conversation (`harness-ask` `last_used`, the
     adapter's `last_used`, CHF-05) must say B;
  5. a reopen with what it says (the flags a chief's reopen passes, headless) answers its
     own turn (its output holds the word asked for), and the reader still says B;
  6. the throwaway's own settings are unchanged by the switch (the key kept it to the
     conversation), and the owner's config and login files are unchanged.

    python3 -I tools/probes/chief-harness/native_switch.py <claude|codex|pi|devin|muse>
        --ask <harness-ask> [--keep] [--explore] [--muse-after-d1]

One harness at a time, in a throwaway config (as resume_matrix.py's), its workspace under
the real home (Devin stalls under the Mac's /var symlink), every CONSENSFLOW_* variable
stripped, logins said "set", never printed. `--explore` prints each screen the driver acts
on; `--keep` keeps the throwaway. Mac only: the keys are T-445's, read on the Mac.
"""
import difflib, glob, hashlib, json, os, re, shutil, sqlite3, subprocess, sys, tempfile, time, uuid

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tuidrive  # noqa: E402

HARNESS = sys.argv[1] if len(sys.argv) > 1 else ""
HOME = os.path.expanduser("~")
EXPLORE = "--explore" in sys.argv


def flag(name, default=None):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


# Each harness's cheap models (operations/test-models.md): A, then B.
MODELS = {
    "claude": ("claude-sonnet-5-5", "claude-haiku-5-5"),
    "codex": ("gpt-5.6-luna", "gpt-6-luna"),
    "pi": ("openai-codex/gpt-5.6-luna", "openai-codex/gpt-6-luna"),
    "devin": ("swe-2-medium", "swe-2-high"),
    "muse": ("muse-spark-1.3", "muse-spark-1.2"),
}
# The owner's files a switch could write: their hashes before and after. Not ~/.claude.json:
# the owner's own Claude Code windows write it all day.
OWNER = [
    "~/.claude/settings.json",
    "~/.codex/config.toml", "~/.codex/auth.json",
    "~/.pi/agent/settings.json", "~/.pi/agent/auth.json",
    "~/.config/devin/config.json", "~/.local/share/devin/credentials.toml",
    "~/.config/muse/settings.json", "~/.config/muse/auth.json",
]
ACCOUNT_WORDS = ("Logged in as", "Organization", "@")
DROPPED = ("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_", "OPENAI_", "CODEX_", "PI_")


def digest(path):
    try:
        with open(os.path.expanduser(path), "rb") as fh:
            return hashlib.sha256(fh.read()).hexdigest()[:12]
    except OSError:
        return None


def read(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return fh.read()
    except OSError:
        return ""


def clean_env(**extra):
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith(DROPPED)}
    env.update(extra)
    return env


def copy_secret(source, target):
    os.makedirs(os.path.dirname(target), exist_ok=True)
    shutil.copyfile(source, target)
    os.chmod(target, 0o600)
    print("  login copied:", os.path.basename(source), "set")


def exe(name):
    found = shutil.which(name)
    if not found:
        sys.exit("%s is not installed here" % name)
    return found


def show(drive, title):
    if EXPLORE:
        print("   --- %s" % title)
        for line in drive.text():
            # A banner can name the account (Devin's "Organization: …"): never printed.
            if line.strip() and not any(word in line for word in ACCOUNT_WORDS):
                print("   |", line.rstrip()[:150])


def settle(drive, title, quiet=2, maxwait=30):
    drive.wait_quiet(quiet=quiet, maxwait=maxwait)
    show(drive, title)


def turn(drive, words, maxwait=240):
    """Asks the window for `words` and waits for its answer and its quiet: true where the
    words came back (once typed, once answered)."""
    count = lambda: sum(line.count(words) for line in drive.text(True))
    before = count()
    drive.type("Reply with exactly: " + words, enter=True)
    start = time.time()
    while count() <= before + 1 and time.time() - start < maxwait:
        drive.pump(0.5)
    settle(drive, "after the turn " + words, quiet=4, maxwait=90)
    return count() > before + 1


def cursor_to(drive, words, marker, way="down", limit=40):
    """Moves a picker's cursor `way` to the row that says `words` in any case: the last row
    `marker` matches (an earlier one is the conversation's own)."""
    for _ in range(limit):
        at = [row for row in drive.text() if re.search(marker, row)]
        if at and words.lower() in at[-1].lower():
            return True
        drive.key(way, settle=0.5)
    return False


def headless(argv, env, cwd):
    """The exit code of a headless run, and whether its output holds the word it was asked
    to reply with (THREE): the reopen answered, not only ended."""
    done = subprocess.run(argv, env=env, cwd=cwd, input="", capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=300)
    return done.returncode, "THREE" in (done.stdout or "")


# --- each harness: its throwaway, window, switch, session and reopen --------------------

def claude(scratch, work):
    config = os.path.join(scratch, "claude")
    os.makedirs(config)
    import getpass
    raw = subprocess.run(["security", "find-generic-password", "-s", "Claude Code-credentials",
                          "-a", getpass.getuser(), "-w"], capture_output=True, text=True,
                         timeout=20).stdout.strip()
    oauth = json.loads(raw)["claudeAiOauth"]
    left = (oauth["expiresAt"] / 1000 - time.time()) / 3600
    if left < 2:
        sys.exit("refusing: the Claude login expires within 2 h (%.1f h)" % left)
    with open(os.open(os.path.join(config, ".credentials.json"), os.O_WRONLY | os.O_CREAT, 0o600), "w") as fh:
        json.dump({"claudeAiOauth": oauth}, fh)
    print("  login copied: claudeAiOauth set")
    del raw, oauth
    # Onboarding done and the workspace trusted, as a real config has them.
    with open(os.path.join(config, ".claude.json"), "w") as fh:
        json.dump({"hasCompletedOnboarding": True, "theme": "dark",
                   "projects": {work: {"hasTrustDialogAccepted": True}}}, fh)
    with open(os.path.join(config, "settings.json"), "w") as fh:
        fh.write("{}\n")
    session = str(uuid.uuid4())
    a, _ = MODELS["claude"]

    def switch(drive):
        drive.type("/model", enter=True, settle=2)
        settle(drive, "the /model picker")
        if not cursor_to(drive, "Haiku", r"❯ \d+\."):
            return False
        # `s`: this conversation only; Enter would make it the default.
        drive.send(b"s")
        time.sleep(2)
        settle(drive, "after s")
        # A conversation with a turn asks first: its cache is the old model's.
        if any("Switch model?" in row for row in drive.text()):
            drive.key("enter", settle=2)
            settle(drive, "after Yes")
        return any("for this session only" in row for row in drive.text(True))

    return {
        "env": clean_env(CLAUDE_CONFIG_DIR=config),
        "window": [exe("claude"), "--strict-mcp-config", "--no-chrome", "--model", a,
                   "--effort", "low", "--session-id", session],
        "switch": switch,
        "session": lambda: session,
        "reopen": lambda used: [exe("claude"), "-p", "--strict-mcp-config", "--no-chrome",
                                "--resume", session, "--model", used["model"]]
        + (["--effort", used["effort"]] if used.get("effort") else []) + ["Reply with exactly: THREE"],
        "settings": [os.path.join(config, "settings.json")],
    }


def codex(scratch, work):
    home = os.path.join(scratch, "codex")
    copy_secret(os.path.join(HOME, ".codex", "auth.json"), os.path.join(home, "auth.json"))
    with open(os.path.join(home, "config.toml"), "w") as fh:
        fh.write('[projects."%s"]\ntrust_level = "trusted"\n' % work)
    a, b = MODELS["codex"]

    def switch(drive):
        drive.type("/model", enter=True, settle=2)
        settle(drive, "the /model list")
        # The list opens on the current model, its last row.
        if not cursor_to(drive, b, r"› \d+\.", way="up"):
            return False
        drive.key("enter", settle=2)
        settle(drive, "the reasoning list")
        # The effort under the cursor (the current one), kept to the conversation by `s`.
        drive.send(b"s")
        time.sleep(2)
        settle(drive, "after s")
        return True

    def session():
        found = sorted(glob.glob(os.path.join(home, "sessions", "*", "*", "*", "rollout-*.jsonl")))
        match = re.search(r"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.jsonl$",
                          found[-1]) if found else None
        return match.group(1) if match else None

    return {
        "env": clean_env(CODEX_HOME=home),
        "window": [exe("codex"), "--model", a, "-c", 'model_reasoning_effort="low"'],
        "switch": switch,
        "session": session,
        "reopen": lambda used: [exe("codex"), "exec", "resume", "--skip-git-repo-check", "--json",
                                "-m", used["model"]]
        + (["-c", 'model_reasoning_effort="%s"' % used["effort"]] if used.get("effort") else [])
        + [session(), "Reply with exactly: THREE"],
        "settings": [os.path.join(home, "config.toml")],
    }


def pi(scratch, work):
    agent = os.path.join(scratch, "pi-agent")
    for name in ("auth.json", "models-store.json", "models.json"):
        source = os.path.join(HOME, ".pi", "agent", name)
        if os.path.exists(source):
            copy_secret(source, os.path.join(agent, name))
    # The owner's settings are not copied: a sessionDir there would take the session home.
    with open(os.path.join(agent, "settings.json"), "w") as fh:
        fh.write("{}\n")
    sessions = os.path.join(agent, "sessions")
    a, b = MODELS["pi"]

    def switch(drive):
        drive.type("/model", enter=True, settle=2)
        settle(drive, "the /model selector")
        # The selector filters as it is typed to; Enter takes it for this session only.
        provider, model = b.split("/", 1)
        drive.type(model, enter=False, settle=1.5)
        settle(drive, "filtered")
        # Another provider's model of that name can come first.
        if not cursor_to(drive, "%s [%s]" % (model, provider), r"→ "):
            return False
        drive.key("enter", settle=2)
        settle(drive, "after Enter")
        return True

    def session():
        found = glob.glob(os.path.join(sessions, "**", "*.jsonl"), recursive=True)
        if not found:
            return None
        for line in open(found[0], encoding="utf-8"):
            entry = json.loads(line)
            if entry.get("type") == "session":
                return entry.get("id")
        return None

    return {
        "env": clean_env(PI_CODING_AGENT_DIR=agent, PI_OFFLINE="1"),
        "window": [exe("pi"), "--no-extensions", "--no-mcp", "--offline", "--model", a,
                   "--thinking", "minimal"],
        "switch": switch,
        "session": session,
        "reopen": lambda used: [exe("pi"), "-p", "--no-extensions", "--no-mcp", "--offline",
                                "--session", session(), "--model", used["model"]]
        + (["--thinking", used["thinking"]] if used.get("thinking") else []) + ["Reply with exactly: THREE"],
        "settings": [os.path.join(agent, "settings.json")],
    }


def devin(scratch, work):
    own = os.path.join(scratch, "home")
    copy_secret(os.path.join(HOME, ".local", "share", "devin", "credentials.toml"),
                os.path.join(own, ".local", "share", "devin", "credentials.toml"))
    store = os.path.join(own, ".local", "share", "devin", "cli", "sessions.db")
    config = os.path.join(own, ".config", "devin", "config.json")
    a, _ = MODELS["devin"]

    def switch(drive):
        drive.type("/model", enter=True, settle=2)
        settle(drive, "the /model list")
        # The family stays; → takes its effort from medium to high; opt+↵ keeps it to the
        # session (↵ would make it the default).
        drive.key("right", settle=1)
        settle(drive, "after →")
        drive.send(b"\x1b\r")
        time.sleep(2)
        settle(drive, "after opt+↵")
        return True

    def session():
        if not os.path.exists(store):
            return None
        db = sqlite3.connect("file:%s?mode=ro" % store, uri=True)
        try:
            row = db.execute("SELECT id FROM sessions ORDER BY created_at DESC LIMIT 1").fetchone()
        finally:
            db.close()
        return row[0] if row else None

    return {
        "env": clean_env(HOME=own, XDG_CONFIG_HOME=os.path.join(own, ".config"),
                         XDG_DATA_HOME=os.path.join(own, ".local", "share")),
        "window": [exe("devin"), "--respect-workspace-trust", "false", "--model", a],
        # Its input line: what is typed before it is up can be lost (one run in three).
        "ready": r"Ask Devin to",
        "switch": switch,
        "session": session,
        "reopen": lambda used: [exe("devin"), "--respect-workspace-trust", "false", "-r", session(),
                                "--model", used["model"], "-p", "Reply with exactly: THREE"],
        "settings": [config],
    }


def muse(scratch, work):
    """Muse's row (the Muse round of T-729): its window on throwaway XDG folders, its login
    the Keychain's (`auth.json` names the storage, never holds a secret), as muse_model.py.
    Its `/model` has no key that keeps a switch to one conversation: Enter saves the model
    as the default, here the throwaway's `settings.json`, which step 6 says. No headless
    resume takes a prompt, so its reopen is a window too. Run it only once the live kit's
    Muse row is on this base (integration D1: a run wrote into the owner's ~/.config/muse)."""
    if "--muse-after-d1" not in sys.argv:
        sys.exit("refusing: Muse's live check waits for the kit's Muse isolation row (D1); "
                 "pass --muse-after-d1 once it is on this base")
    auth = json.load(open(os.path.join(HOME, ".config", "muse", "auth.json")))
    storages = [provider.get("storage") for provider in (auth.get("providers") or {}).values()]
    if not storages or any(storage != "keychain" for storage in storages):
        sys.exit("refusing: a Muse login is kept in its file, which a copy would carry")
    del auth
    config, data = os.path.join(scratch, "config"), os.path.join(scratch, "data")
    os.makedirs(os.path.join(config, "muse"))
    shutil.copy2(os.path.join(HOME, ".config", "muse", "auth.json"), os.path.join(config, "muse", "auth.json"))
    print("  login copied: auth.json (the Keychain's) set")
    os.makedirs(os.path.join(data, "muse"))
    env = clean_env(XDG_CONFIG_HOME=config, XDG_DATA_HOME=data,
                    XDG_CACHE_HOME=os.path.join(scratch, "cache"),
                    XDG_STATE_HOME=os.path.join(scratch, "state"), MUSE_NO_AUTO_UPDATE="1")
    a, b = MODELS["muse"]
    base = [exe("muse"), "--contributor-mode", "off"]

    def switch(drive):
        drive.type("/model", enter=True, settle=2)
        settle(drive, "the /model picker")
        if not cursor_to(drive, b, r"[›❯>] "):
            return False
        # Enter: Muse's only key, which saves the default.
        drive.key("enter", settle=2)
        settle(drive, "after Enter")
        # The effort picker that follows: left as it stands.
        drive.key("esc", settle=1.5)
        settle(drive, "after Esc")
        return True

    def session():
        import sqlite3
        index = os.path.join(data, "muse", "session-index.db")
        if not os.path.exists(index):
            return None
        db = sqlite3.connect("file:%s?mode=ro" % index, uri=True)
        try:
            row = db.execute("SELECT session_id FROM sessions ORDER BY rowid DESC LIMIT 1").fetchone()
        finally:
            db.close()
        return row[0] if row else None

    return {
        "env": env,
        "window": base + ["--model", a, "--reasoning-effort", "minimal"],
        "switch": switch,
        "session": session,
        "reopen_window": lambda used: base + ["--model", used["model"], "resume", session()],
        "settings": [os.path.join(config, "muse", "settings.json")],
    }


def reopened(argv, env, cwd):
    """A window opened on `argv` answers one turn (THREE): the reopen of a harness that
    takes no prompt headless."""
    drive = tuidrive.Session(argv, env, cwd, limit=600)
    try:
        settle(drive, "the reopened window", quiet=4, maxwait=90)
        return 0, turn(drive, "THREE")
    finally:
        drive.key("ctrl-c", settle=0.5)
        drive.key("ctrl-c", settle=2)
        drive.kill()


def ask(binary, kind, session, env):
    asked = subprocess.run([binary], input=json.dumps({"op": "last_used", "kind": kind,
                                                      "session": session, "env": env}),
                           capture_output=True, text=True, timeout=60)
    try:
        return json.loads(asked.stdout)["answered"]
    except (ValueError, KeyError):
        return {"unread": (asked.stderr or asked.stdout)[:300]}


def main():
    if HARNESS not in MODELS or not flag("--ask"):
        sys.exit(__doc__)
    binary = os.path.abspath(flag("--ask"))
    kind = {"claude": "claude-code"}.get(HARNESS, HARNESS)
    before = {path: digest(path) for path in OWNER}
    scratch = tempfile.mkdtemp(prefix=".cf-native-switch-", dir=HOME)
    work = os.path.join(scratch, "work")
    os.makedirs(work)
    a, b = MODELS[HARNESS]
    print("%s: A %s, B %s" % (HARNESS, a, b))
    try:
        it = globals()[HARNESS](scratch, work)
        settings = {path: read(path) for path in it["settings"]}
        drive = tuidrive.Session(it["window"], it["env"], work, limit=1200,
                                 rawlog=os.path.join(scratch, "window.raw"))
        try:
            if it.get("ready"):
                drive.wait_for(it["ready"], maxwait=120, history=False)
            settle(drive, "the window", quiet=4, maxwait=90)
            print("1. a turn on A answered:", turn(drive, "ONE"))
            print("2. the native switch made:", it["switch"](drive))
            print("3. a turn on B answered:", turn(drive, "TWO"))
            drive.key("ctrl-c", settle=0.5)
            drive.key("ctrl-c", settle=2)
        finally:
            drive.kill()
        session = it["session"]()
        used = ask(binary, kind, session or "none", it["env"])
        print("4. ConsensFlow's reader says:", json.dumps(used), "| B:", isinstance(used, dict) and used.get("model") == b)
        if isinstance(used, dict) and used.get("model"):
            if "reopen_window" in it:
                code, answered = reopened(it["reopen_window"](used), it["env"], work)
            else:
                code, answered = headless(it["reopen"](used), it["env"], work)
            again = ask(binary, kind, session, it["env"])
            print("5. the reopen with it exited %s and answered THREE: %s; the reader then says: %s"
                  % (code, answered, json.dumps(again)))
        for path, was in settings.items():
            now = read(path)
            print("6. the throwaway's %s unchanged: %s" % (os.path.basename(path), now == was))
            # What the harness wrote there of a model, an effort or a thinking level; the
            # rest is counted, never printed (Devin writes its account's organization there).
            changed = [line for line in difflib.unified_diff(was.splitlines(), now.splitlines(), lineterm="", n=0)
                       if not line.startswith(("---", "+++", "@@"))]
            said = [line for line in changed if re.search(r"model|effort|thinking", line, re.I)]
            for line in said:
                print("     ", line[:160])
            print("      (%d other changed lines, none of a model)" % (len(changed) - len(said)))
    finally:
        after = {path: digest(path) for path in OWNER}
        print("   the owner's files unchanged:", after == before,
              [path for path in OWNER if after[path] != before[path]] or "")
        if "--keep" in sys.argv:
            print("   kept:", scratch)
        else:
            shutil.rmtree(scratch, ignore_errors=True)


main()
