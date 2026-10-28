#!/usr/bin/env python3 -I
"""T-445's resume matrix replayed headless, on the Mac and on Windows (CHF-02): what each
harness's own record says of the model a conversation runs on when it is resumed with no
model flag, with another model's flag, and with a model the harness does not have.

    python3 -I tools/probes/chief-harness/resume_matrix.py <claude|codex|pi|devin>
        [--a MODEL] [--b MODEL] [--timeout SECONDS] [--in FOLDER]
    (zeewin2: ssh zeewin2 'C:\\Python314\\python.exe -I - <harness> [flags]' < resume_matrix.py)

Each harness runs one conversation in a throwaway config, headless (its print mode), with
one short turn at each step, on its cheap test model (operations/test-models.md):

  start     on model A (and a low effort where the harness takes one as a flag)
  R1        resumed with no model flag
  R2        resumed with --model B
  R3        resumed with --model of a model that does not exist
  (Devin)   R4: a new conversation started with --model <family> and no effort (O2)

After each step it reads the conversation's own record the way CHF-05 will (Claude: the
last non-sidechain assistant entry; Codex: the last turn_context; Pi: the last assistant
message; Devin: sessions.model) and prints that, the exit code, and the first words of
any failure. A turn's text is never printed, and a login never: each login is copied into
the throwaway with the mode 600 and said "set", the owner's file hashed before and after.
Every CONSENSFLOW_* variable is stripped from what a harness is given.

The TUI's own switch (`/model`) is not driven here: T-445 ran it on the Mac
(.runs/T-model-switch); a headless resume is the same resume, minus the screen.
"""
import glob, hashlib, json, os, shutil, sqlite3, subprocess, sys, tempfile, time, uuid

WINDOWS = sys.platform == "win32"
HARNESS = sys.argv[1] if len(sys.argv) > 1 else ""


def flag(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


LIMIT = int(flag("--timeout", "240"))
CHEAP = {
    "claude": ("claude-sonnet-5-5", "claude-haiku-5-5"),
    "codex": ("gpt-5.6-luna", "gpt-6-luna"),
    "pi": ("openrouter/meta/muse-spark-1.3", "openai-codex/gpt-5.6-luna"),
    "devin": ("swe-2-medium", "swe-2-high"),
}
GONE = {
    "claude": "claude-gone-9-9",
    "codex": "gpt-gone-9",
    "pi": "openai/gpt-gone-9",
    "devin": "swe-gone-9-medium",
}
PROMPT = "Reply with exactly: CF_OK"
HOME = os.path.expanduser("~")
# The words of a line that names an account, which the probe never prints.
ACCOUNT_WORDS = ("Logged in as", "Organization", "@")
DROPPED = ("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_", "OPENAI_", "CODEX_", "PI_")


def clean_env(**extra):
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith(DROPPED)}
    env.update(extra)
    return env


def digest(path):
    try:
        with open(path, "rb") as fh:
            return hashlib.sha256(fh.read()).hexdigest()[:12]
    except OSError:
        return None


def copy_secret(source, target):
    os.makedirs(os.path.dirname(target), exist_ok=True)
    shutil.copyfile(source, target)
    os.chmod(target, 0o600)
    print("  login copied:", os.path.basename(source), "set")


def remove(path):
    for _ in range(10):
        shutil.rmtree(path, ignore_errors=True)
        if not os.path.exists(path):
            return
        time.sleep(0.5)


def run(argv, env, cwd, stdin=None):
    started = time.time()
    try:
        done = subprocess.run(argv, env=env, cwd=cwd, input=stdin or "", capture_output=True,
                              text=True, encoding="utf-8", errors="replace", timeout=LIMIT)
        code, out, err = done.returncode, done.stdout, done.stderr
    except subprocess.TimeoutExpired as expired:
        code, out, err = "timeout", expired.stdout or "", expired.stderr or ""
        if isinstance(out, bytes):
            out, err = out.decode(errors="replace"), (err or b"").decode(errors="replace")
    return {"code": code, "seconds": round(time.time() - started, 1), "out": out, "err": err}


def failure(ran):
    """The first words of a failure: the error items of a JSON stream (Codex's), else the
    error output, else the output's error lines."""
    said = []
    for line in (ran["out"] or "").splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        item = event.get("item") if isinstance(event, dict) else None
        if isinstance(item, dict) and item.get("type") == "error":
            said.append(item.get("message", ""))
        elif isinstance(event, dict) and event.get("type") in ("error", "turn.failed"):
            said.append(json.dumps(event.get("message") or event.get("error"))[:200])
    if said:
        return " | ".join(said)[:600]
    # A harness's banner can name the account (Devin's "Logged in as …"): never said.
    text = "\n".join(line for line in (ran["err"] or "").splitlines()
                     if not any(word in line for word in ACCOUNT_WORDS)).strip()
    if not text:
        lines = [l for l in (ran["out"] or "").splitlines() if "error" in l.lower()]
        text = " | ".join(lines[:2])
    return text.replace("\n", " | ")[:300]


def exe(name):
    found = shutil.which(name)
    if not found:
        sys.exit("%s is not installed" % name)
    return found


# --- Claude ---------------------------------------------------------------------------

def claude(scratch, a, b):
    config = os.path.join(scratch, "claude")
    os.makedirs(config)
    if WINDOWS:
        raw = open(os.path.join(HOME, ".claude", ".credentials.json")).read()
    else:
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
    print("  login copied: claudeAiOauth set (%.1f h left)" % left)
    del raw, oauth
    env = clean_env(CLAUDE_CONFIG_DIR=config)
    work = os.path.join(scratch, "work")
    os.makedirs(work)
    session = str(uuid.uuid4())
    base = [exe("claude"), "-p", "--strict-mcp-config", "--no-chrome", "--output-format", "json"]

    def record():
        found = glob.glob(os.path.join(config, "projects", "*", session + ".jsonl"))
        if not found:
            return {"record": "none"}
        last = None
        for line in open(found[0], encoding="utf-8"):
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            if entry.get("type") == "assistant" and not entry.get("isSidechain"):
                message = entry.get("message") or {}
                last = {"model": message.get("model"), "effort": entry.get("effort"),
                        "requestedModel": entry.get("requestedModel"),
                        "error": entry.get("isApiErrorMessage") or entry.get("error")}
        return last or {"record": "no assistant entry"}

    steps = [
        ("start", base + ["--model", a, "--effort", "low", "--session-id", session, PROMPT]),
        ("R1 no flag", base + ["--resume", session, PROMPT]),
        ("R2 --model B", base + ["--resume", session, "--model", b, PROMPT]),
        ("R3 gone model", base + ["--resume", session, "--model", GONE["claude"], PROMPT]),
        ("R1' no flag after", base + ["--resume", session, PROMPT]),
    ]
    for name, argv in steps:
        yield name, run(argv, env, work), record()


# --- Codex ----------------------------------------------------------------------------

def codex(scratch, a, b):
    home = os.path.join(scratch, "codex")
    copy_secret(os.path.join(HOME, ".codex", "auth.json"), os.path.join(home, "auth.json"))
    env = clean_env(CODEX_HOME=home)
    work = os.path.join(scratch, "work")
    os.makedirs(work)
    base = [exe("codex"), "exec", "--skip-git-repo-check", "--json"]
    thread = {}

    def record():
        found = glob.glob(os.path.join(home, "sessions", "*", "*", "*", "rollout-*%s.jsonl" % thread.get("id", "?")))
        if not found:
            return {"record": "none"}
        last = None
        for line in open(found[0], encoding="utf-8"):
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            payload = entry.get("payload") or {}
            if entry.get("type") == "turn_context":
                last = {"model": payload.get("model"), "effort": payload.get("effort")}
        return last or {"record": "no turn_context"}

    started = run(base + ["-m", a, "-c", 'model_reasoning_effort="low"', PROMPT], env, work)
    for line in started["out"].splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("type") == "thread.started":
            thread["id"] = event.get("thread_id")
    yield "start", started, record()
    resume = [exe("codex"), "exec", "resume", "--skip-git-repo-check", "--json", thread.get("id", "none")]
    for name, extra in [("R1 no flag", []), ("R2 --model B", ["-m", b]),
                        ("R3 gone model", ["-m", GONE["codex"]]), ("R1' no flag after", [])]:
        yield name, run(resume[:3] + extra + resume[3:] + [PROMPT], env, work), record()


# --- Pi -------------------------------------------------------------------------------

def pi(scratch, a, b):
    agent = os.path.join(scratch, "pi-agent")
    for name in ("auth.json", "models-store.json", "models.json", "settings.json"):
        source = os.path.join(HOME, ".pi", "agent", name)
        if os.path.exists(source):
            copy_secret(source, os.path.join(agent, name))
    sessions = os.path.join(scratch, "pi-sessions")
    env = clean_env(PI_CODING_AGENT_DIR=agent, PI_OFFLINE="1")
    work = os.path.join(scratch, "work")
    os.makedirs(work)
    session = str(uuid.uuid4())
    base = [exe("pi"), "-p", "--no-extensions", "--no-mcp", "--offline", "--session-dir", sessions,
            "--session-id", session]

    def record():
        found = glob.glob(os.path.join(sessions, "**", "*.jsonl"), recursive=True)
        if not found:
            return {"record": "none"}
        last, changes = None, []
        for line in open(found[0], encoding="utf-8"):
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            message = entry.get("message") or {}
            if entry.get("type") == "message" and message.get("role") == "assistant":
                last = {"provider": message.get("provider"), "model": message.get("model"),
                        "thinkingLevel": message.get("thinkingLevel") or entry.get("thinkingLevel"),
                        "stopReason": message.get("stopReason"),
                        "error": (message.get("errorMessage") or "")[:160] or None}
            if entry.get("type") in ("model_change", "thinking_level_change"):
                changes.append({k: entry.get(k) for k in ("type", "provider", "modelId", "thinkingLevel")})
        return {"last assistant": last, "changes": changes[-2:]}

    steps = [
        ("start", base + ["--model", a, "--thinking", "minimal", PROMPT]),
        ("R1 no flag", base + [PROMPT]),
        ("R2 --model B", base + ["--model", b, PROMPT]),
        ("R3 gone model", base + ["--model", GONE["pi"], PROMPT]),
        ("R1' no flag after", base + [PROMPT]),
    ]
    for name, argv in steps:
        yield name, run(argv, env, work), record()


# --- Devin ----------------------------------------------------------------------------

def devin(scratch, a, b):
    own = os.path.join(scratch, "home")
    if WINDOWS:
        roaming = os.path.join(scratch, "roaming")
        copy_secret(os.path.join(os.environ["APPDATA"], "devin", "credentials.toml"),
                    os.path.join(roaming, "devin", "credentials.toml"))
        env = clean_env(APPDATA=roaming)
        store = os.path.join(roaming, "devin", "cli", "sessions.db")
    else:
        copy_secret(os.path.join(HOME, ".local", "share", "devin", "credentials.toml"),
                    os.path.join(own, ".local", "share", "devin", "credentials.toml"))
        env = clean_env(HOME=own, XDG_CONFIG_HOME=os.path.join(own, ".config"),
                        XDG_DATA_HOME=os.path.join(own, ".local", "share"))
        store = os.path.join(own, ".local", "share", "devin", "cli", "sessions.db")
    work = os.path.join(scratch, "work")
    os.makedirs(work)
    newest = {}
    # As ConsensFlow launches Devin: print mode cannot ask about the folder's trust.
    devin_ = [exe("devin"), "--respect-workspace-trust", "false"]

    def sessions():
        if not os.path.exists(store):
            return []
        db = sqlite3.connect("file:%s?mode=ro" % store, uri=True)
        try:
            return db.execute("SELECT id, model FROM sessions ORDER BY created_at").fetchall()
        finally:
            db.close()

    def record():
        rows = sessions()
        model = dict(rows).get(newest.get("id"))
        return {"sessions.model": model, "sessions": len(rows)}

    started = run(devin_ + ["-p", PROMPT, "--model", a], env, work)
    rows = sessions()
    newest["id"] = rows[-1][0] if rows else None
    yield "start", started, record()
    for name, extra in [("R1 no flag", []), ("R2 --model B", ["--model", b]),
                        ("R3 gone model", ["--model", GONE["devin"]]), ("R1' no flag after", [])]:
        ran = run(devin_ + ["-r", newest["id"] or "none", "-p", PROMPT] + extra, env, work)
        yield name, ran, record()
    family = a.rsplit("-", 1)[0]
    before = {row[0] for row in sessions()}
    ran = run(devin_ + ["-p", PROMPT, "--model", family], env, work)
    fresh = [row for row in sessions() if row[0] not in before]
    newest["id"] = fresh[-1][0] if fresh else None
    yield "R4 --model %s (no effort)" % family, ran, record()


def main():
    reads = {"claude": claude, "codex": codex, "pi": pi, "devin": devin}
    if HARNESS not in reads:
        sys.exit(__doc__)
    a, b = flag("--a", CHEAP[HARNESS][0]), flag("--b", CHEAP[HARNESS][1])
    logins = [os.path.join(HOME, ".codex", "auth.json"), os.path.join(HOME, ".pi", "agent", "auth.json"),
              os.path.join(HOME, ".local", "share", "devin", "credentials.toml"),
              os.path.join(HOME, ".claude", ".credentials.json")]
    before = [digest(path) for path in logins]
    # A folder under the real home where one is named (`--in`): Devin stalls in a workspace
    # under the Mac's /var symlink.
    scratch = tempfile.mkdtemp(prefix="cf-resume-matrix-", dir=flag("--in", None))
    print("%s on %s: A %s, B %s, gone %s" % (HARNESS, sys.platform, a, b, GONE[HARNESS]))
    summary = []
    try:
        for name, ran, said in reads[HARNESS](scratch, a, b):
            ok = ran["code"] == 0
            line = {"step": name, "exit": ran["code"], "seconds": ran["seconds"], "record": said}
            if not ok:
                line["failure"] = failure(ran)
            summary.append(line)
            print(" ", json.dumps(line))
    finally:
        remove(scratch)
    after = [digest(path) for path in logins]
    print("throwaway removed:", not os.path.exists(scratch), "| owner's logins unchanged:", before == after)


main()
