#!/usr/bin/env python3 -I
"""What each harness lists of its models, read the way ConsensFlow reads it (CHF-03),
with no model turn, under a throwaway config: never the owner's own.

    python3 -I tools/probes/chief-harness/models.py <claude|codex|pi|devin>
        [--signed-out] [--record FILE] [--timeout SECONDS]

- claude: `claude -p --input-format stream-json`, the SDK's `initialize` control
  request, in an empty CLAUDE_CONFIG_DIR (signed out: the list is the same signed in,
  research-02 §4, and that is the product's own route).
- codex: `codex app-server`, `initialize` then `model/list`, in a CODEX_HOME holding a
  copy of `~/.codex/auth.json` (none with --signed-out: the bundled list).
- pi: `pi --mode rpc --offline`, `get_available_models`, in a PI_CODING_AGENT_DIR
  holding copies of `~/.pi/agent`'s auth, catalog and settings.
- devin: `devin models list --format json`, under a throwaway HOME holding a copy of
  `~/.local/share/devin/credentials.toml` (none with --signed-out).

Prints counts, ids and timings only; a login is said "set", never shown. Each login file
copied is hashed before and after, so a run that rotated the owner's token says so.
--record writes the answer to FILE, scrubbed (Claude's `account` block dropped) and
sampled (Pi's and Devin's lists are 300-400 KB: a few rows of each shape are kept), as
the fixtures of `crates/cf-harness/tests/fixtures/models/` were made. Every
CONSENSFLOW_* variable is stripped from what the harness is given.
"""
import hashlib, json, os, queue, shutil, subprocess, sys, tempfile, threading, time

HARNESS = sys.argv[1] if len(sys.argv) > 1 else ""
SIGNED_OUT = "--signed-out" in sys.argv
RECORD = sys.argv[sys.argv.index("--record") + 1] if "--record" in sys.argv else None
LIMIT = int(sys.argv[sys.argv.index("--timeout") + 1]) if "--timeout" in sys.argv else 40
HOME = os.path.expanduser("~")
DROPPED = ("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_", "OPENAI_", "CODEX_", "PI_")


def clean_env(**extra):
    env = {k: v for k, v in os.environ.items() if not k.startswith(DROPPED)}
    env.update(extra)
    return env


def digest(path):
    if not os.path.exists(path):
        return None
    with open(path, "rb") as fh:
        return hashlib.sha256(fh.read()).hexdigest()[:12]


def copy_secret(source, target):
    os.makedirs(os.path.dirname(target), exist_ok=True)
    shutil.copyfile(source, target)
    os.chmod(target, 0o600)
    print("login copied:", os.path.basename(source), "set")


def remove(path):
    for _ in range(10):
        shutil.rmtree(path, ignore_errors=True)
        if not os.path.exists(path):
            return
        time.sleep(0.5)


def stop(proc):
    if sys.platform == "win32":
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(proc.pid)], capture_output=True)
    else:
        proc.kill()
    proc.wait()


class Lines:
    """A child's output a line at a time, read on a thread, bounded by the deadline."""

    def __init__(self, argv, env, cwd):
        self.proc = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, env=env, cwd=cwd, text=True,
                                     bufsize=1)
        self.lines = queue.Queue()
        self.deadline = time.time() + LIMIT
        threading.Thread(target=self.pump, daemon=True).start()

    def pump(self):
        for line in self.proc.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def send(self, message):
        self.proc.stdin.write(json.dumps(message) + "\n")
        self.proc.stdin.flush()

    def answer(self, wanted):
        while time.time() < self.deadline:
            try:
                line = self.lines.get(timeout=0.5)
            except queue.Empty:
                continue
            if line is None:
                return None
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if wanted(message):
                return message
        return None


def claude(scratch):
    config = os.path.join(scratch, "config")
    os.makedirs(config)
    env = clean_env(CLAUDE_CONFIG_DIR=config, CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1")
    child = Lines([shutil.which("claude"), "-p", "--input-format", "stream-json",
                   "--output-format", "stream-json", "--verbose", "--strict-mcp-config",
                   "--no-chrome", "--no-session-persistence"], env, scratch)
    try:
        child.send({"type": "control_request", "request_id": "models",
                    "request": {"subtype": "initialize"}})
        got = child.answer(lambda m: m.get("type") == "control_response"
                           and m.get("response", {}).get("request_id") == "models")
    finally:
        stop(child.proc)
    if got is None:
        return None
    result = got["response"].get("response", {})
    kept = {k: result[k] for k in ("models", "unavailable_models") if k in result}
    print("rows:", [(m.get("value"), m.get("resolvedModel"), m.get("supportedEffortLevels"))
                    for m in kept.get("models", [])])
    return kept


def codex(scratch):
    home = os.path.join(scratch, "codex-home")
    os.makedirs(home)
    real = os.path.join(HOME, ".codex", "auth.json")
    if not SIGNED_OUT:
        copy_secret(real, os.path.join(home, "auth.json"))
    child = Lines([shutil.which("codex"), "app-server"], clean_env(CODEX_HOME=home), scratch)
    try:
        child.send({"id": 1, "method": "initialize",
                    "params": {"clientInfo": {"name": "consensflow-probe", "version": "0"},
                               "capabilities": {"experimentalApi": True}}})
        if child.answer(lambda m: m.get("id") == 1) is None:
            return None
        child.send({"method": "initialized"})
        child.send({"id": 2, "method": "model/list", "params": {"includeHidden": True}})
        got = child.answer(lambda m: m.get("id") == 2)
    finally:
        stop(child.proc)
    if got is None or "result" not in got:
        print("answer:", got and got.get("error"))
        return None
    result = got["result"]
    print("rows:", [(m.get("model"), m.get("hidden"), m.get("isDefault"),
                     [e.get("reasoningEffort") for e in m.get("supportedReasoningEfforts", [])])
                    for m in result.get("data", [])])
    return result


def pi(scratch):
    agent = os.path.join(scratch, "pi-agent")
    os.makedirs(agent)
    source = os.path.join(HOME, ".pi", "agent")
    for name in ("auth.json", "models-store.json", "models.json", "settings.json"):
        if os.path.exists(os.path.join(source, name)) and not (SIGNED_OUT and name == "auth.json"):
            copy_secret(os.path.join(source, name), os.path.join(agent, name))
    env = clean_env(PI_CODING_AGENT_DIR=agent, PI_OFFLINE="1", HOME=scratch)
    child = Lines([shutil.which("pi"), "--mode", "rpc", "--no-session", "--no-extensions",
                   "--no-mcp", "--offline"], env, scratch)
    try:
        child.send({"id": "models", "type": "get_available_models"})
        got = child.answer(lambda m: m.get("type") == "response" and m.get("id") == "models")
    finally:
        stop(child.proc)
    if got is None:
        return None
    models = (got.get("data") or {}).get("models", [])
    by = {}
    for model in models:
        by.setdefault(model.get("provider"), []).append(model)
    print("success:", got.get("success"), "models:", len(models),
          "per provider:", {p: len(v) for p, v in by.items()})
    sample = []
    for provider, rows in sorted(by.items()):
        reasoning = [m for m in rows if m.get("reasoning")][:3]
        plain = [m for m in rows if not m.get("reasoning")][:1]
        sample += reasoning + plain
    return {"type": "response", "command": "get_available_models", "id": "models",
            "success": got.get("success"), "data": {"models": sample}}


def devin(scratch):
    real = os.path.join(HOME, ".local", "share", "devin", "credentials.toml")
    if not SIGNED_OUT:
        copy_secret(real, os.path.join(scratch, ".local", "share", "devin", "credentials.toml"))
    env = clean_env(HOME=scratch, XDG_CONFIG_HOME=os.path.join(scratch, ".config"),
                    XDG_DATA_HOME=os.path.join(scratch, ".local", "share"))
    out = subprocess.run([shutil.which("devin"), "models", "list", "--format", "json"],
                         env=env, cwd=scratch, capture_output=True, text=True, timeout=LIMIT)
    print("exit:", out.returncode, "stderr:", out.stderr.strip()[:200])
    if out.returncode != 0:
        return {"exit": out.returncode, "stderr": out.stderr.strip()[:400]}
    listed = json.loads(out.stdout)
    families = listed.get("families", [])
    print("families:", len(families), "variants:", sum(len(f.get("variants", [])) for f in families))
    kept_families = ("claude-opus-5-5", "gpt-6-sol", "swe-2", "kimi-k3", "adaptive", "fusion",
                     "swe-1-7", "glm-5-2")
    sample = []
    for family in families:
        uid = family.get("family_uid", "")
        if any(uid.startswith(k) or family.get("slug", "").startswith(k) for k in kept_families):
            family = dict(family)
            if uid.startswith("fusion") or family.get("slug", "").startswith("fusion"):
                family["variants"] = family.get("variants", [])[:2]
            sample.append(family)
    print("sampled families:", [f.get("family_uid") for f in sample])
    return {"families": sample}


def main():
    reads = {"claude": claude, "codex": codex, "pi": pi, "devin": devin}
    if HARNESS not in reads:
        sys.exit(__doc__)
    logins = [os.path.join(HOME, ".codex", "auth.json"), os.path.join(HOME, ".pi", "agent", "auth.json"),
              os.path.join(HOME, ".local", "share", "devin", "credentials.toml")]
    before = [digest(path) for path in logins]
    scratch = tempfile.mkdtemp(prefix="cf-probe-models-")
    started = time.time()
    try:
        answer = reads[HARNESS](scratch)
    finally:
        remove(scratch)
    print("seconds: %.1f" % (time.time() - started), "throwaway removed:", not os.path.exists(scratch))
    after = [digest(path) for path in logins]
    print("owner's logins unchanged:", before == after)
    if answer is None:
        print("no answer within", LIMIT, "s")
        sys.exit(1)
    if RECORD:
        with open(RECORD, "w") as fh:
            json.dump(answer, fh, indent=1, sort_keys=True)
            fh.write("\n")
        print("recorded:", RECORD)


main()
