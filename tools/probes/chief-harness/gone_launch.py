#!/usr/bin/env python3 -I
"""A chief's window launched on a model its account does not have (CHF-06, the gone-model
launch after Calliope's T-955 P1 and T-994): does the window end at launch with the words on
its screen, or stay up and fail its first turn in its record? A chief's window counts as
come up where its record shows a message it was handed, and only a launch that ends with a
code of failure before that, within the launch's budget, is read in its screen's words; a
turn that fails is read in its record's own words. So the answer says where a real launch
of each harness on a gone model is heard.

    python3 -I tools/probes/chief-harness/gone_launch.py [claude|codex|pi]

One window at a time, each in a throwaway home, every CONSENSFLOW_* variable stripped, logins
said "set" and never printed, no model turn spent (the model is refused):
- claude: a throwaway CLAUDE_CONFIG_DIR seeded from the Keychain, on `claude-gone-9-9`;
- codex: a throwaway CODEX_HOME holding a copy of the owner's auth.json (refused under 40
  minutes of token life; its hash compared after, so a refresh the copy made is said), on
  `gpt-gone-9`;
- pi: a throwaway HOME (CARGO_HOME and RUSTUP_HOME pinned to the owner's), holding only the
  logins with 40 minutes of life or an API key, on `openai-codex/gpt-gone-9` (OpenAI's
  provider) and then on `anthropic/claude-gone-9` (Anthropic's).
Each prints how the window ended, its screen's last lines, then, where it stayed up, the
same after one turn, and what the session's record holds of the failure.
"""
import base64, glob, hashlib, json, os, shutil, subprocess, sys, tempfile, time, uuid

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tuidrive  # noqa: E402

HOME = os.path.expanduser("~")
DROPPED = ("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_", "OPENAI_", "CODEX_", "PI_")
# Never printed: a banner can name the account.
ACCOUNT_WORDS = ("Logged in as", "Organization", "@")


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


def last_lines(drive, count=8):
    for line in [line for line in drive.text() if line.strip()][-count:]:
        if not any(word in line for word in ACCOUNT_WORDS):
            print("   |", line.rstrip()[:150])


def watch(argv, env, work):
    """Opens the window on `argv`: how it ended within 60 s, its screen, and where it stayed
    up, one turn and its screen after."""
    drive = tuidrive.Session(argv, env, work, limit=240)
    try:
        start = time.time()
        while drive.alive() and time.time() - start < 60:
            drive.pump(0.5)
        alive = drive.alive()
        print("the window ended by itself within 60 s:", not alive)
        print("its exit code:", None if alive else drive.proc.returncode)
        print("its screen's last lines:")
        last_lines(drive)
        if alive:
            # Pi asks to trust the folder first: the throwaway's own workspace is trusted.
            if any("Do not trust" in row for row in drive.text()):
                drive.key("enter", settle=2)
                drive.wait_quiet(quiet=3, maxwait=30)
            drive.type("Reply with exactly: ONE", enter=True)
            drive.wait_quiet(quiet=5, maxwait=120)
            print("after one turn, alive:", drive.alive(),
                  "| exit code:", None if drive.alive() else drive.proc.returncode)
            last_lines(drive)
    finally:
        drive.kill()


def claude():
    gone = "claude-gone-9-9"
    scratch = tempfile.mkdtemp(prefix=".cf-gone-launch-", dir=HOME)
    try:
        config = os.path.join(scratch, "claude")
        work = os.path.join(scratch, "work")
        os.makedirs(config)
        os.makedirs(work)
        import getpass
        raw = subprocess.run(["security", "find-generic-password", "-s", "Claude Code-credentials",
                              "-a", getpass.getuser(), "-w"], capture_output=True, text=True,
                             timeout=20).stdout.strip()
        oauth = json.loads(raw)["claudeAiOauth"]
        # The run takes a few minutes: the login must last that and 30 minutes more, so
        # that the owner's own Claude Code refreshes it, never this copy.
        if (oauth["expiresAt"] / 1000 - time.time()) / 60 < 40:
            sys.exit("refusing: the Claude login expires within 40 minutes")
        with open(os.open(os.path.join(config, ".credentials.json"), os.O_WRONLY | os.O_CREAT, 0o600), "w") as fh:
            json.dump({"claudeAiOauth": oauth}, fh)
        del raw, oauth
        print("login copied: claudeAiOauth set")
        with open(os.path.join(config, ".claude.json"), "w") as fh:
            json.dump({"hasCompletedOnboarding": True, "theme": "dark",
                       "projects": {work: {"hasTrustDialogAccepted": True}}}, fh)
        session = str(uuid.uuid4())
        argv = [shutil.which("claude"), "--strict-mcp-config", "--no-chrome", "--model", gone,
                "--session-id", session]
        watch(argv, clean_env(CLAUDE_CONFIG_DIR=config), work)
        found = glob.glob(os.path.join(config, "projects", "*", session + ".jsonl"))
        print("the session's record:", "none" if not found else os.path.basename(found[0]))
        for path in found:
            for line in open(path, encoding="utf-8"):
                try:
                    entry = json.loads(line)
                except ValueError:
                    continue
                message = entry.get("message") or {}
                if entry.get("type") not in ("user", "assistant", "system"):
                    continue
                said = message.get("content") if isinstance(message.get("content"), str) else " ".join(
                    block.get("text", "") for block in message.get("content") or [] if isinstance(block, dict))
                # A refusal's words are the harness's, never the account's: shown.
                print("   entry:", entry.get("type"), message.get("model"),
                      "apiError" if entry.get("isApiErrorMessage") else "",
                      (said or entry.get("content") or "")[:220] if entry.get("type") != "user" else "")
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
        print("throwaway removed:", not os.path.exists(scratch))


def codex():
    gone = "gpt-gone-9"
    source = os.path.join(HOME, ".codex", "auth.json")
    auth = json.load(open(source, encoding="utf-8"))
    token = (auth.get("tokens") or {}).get("access_token") or ""
    if token:
        part = token.split(".")[1]
        part += "=" * (-len(part) % 4)
        if (json.loads(base64.urlsafe_b64decode(part)).get("exp", 0) - time.time()) / 60 < 40:
            sys.exit("refusing: the Codex login's token expires within 40 minutes")
    del auth, token
    owner = digest(source)
    scratch = tempfile.mkdtemp(prefix=".cf-gone-launch-", dir=HOME)
    try:
        home = os.path.join(scratch, "codex")
        work = os.path.join(scratch, "work")
        os.makedirs(home)
        os.makedirs(work)
        copy = os.path.join(home, "auth.json")
        shutil.copyfile(source, copy)
        os.chmod(copy, 0o600)
        copied = digest(copy)
        print("login copied: auth.json set")
        with open(os.path.join(home, "config.toml"), "w") as fh:
            fh.write('[projects."%s"]\ntrust_level = "trusted"\n' % work)
        watch([shutil.which("codex"), "--model", gone], clean_env(CODEX_HOME=home), work)
        print("the copy's login unchanged (no refresh by the throwaway):", digest(copy) == copied)
        found = sorted(glob.glob(os.path.join(home, "sessions", "*", "*", "*", "rollout-*.jsonl")))
        print("the session's record:", "none" if not found else os.path.basename(found[-1]))
        for path in found[-1:]:
            for line in open(path, encoding="utf-8"):
                try:
                    entry = json.loads(line)
                except ValueError:
                    continue
                payload = entry.get("payload") or {}
                kind = payload.get("type")
                if entry.get("type") == "turn_context":
                    print("   turn_context model:", payload.get("model"))
                elif kind in ("task_started", "task_complete", "error", "stream_error", "turn_aborted"):
                    said = {k: v for k, v in payload.items() if k in ("error", "message", "last_agent_message", "reason")}
                    print("   event:", kind, json.dumps(said)[:300])
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
        print("throwaway removed:", not os.path.exists(scratch))
        print("the owner's auth.json unchanged:", digest(source) == owner)


def pi():
    source = os.path.join(HOME, ".pi", "agent", "auth.json")
    owner = digest(source)
    logins = json.load(open(source, encoding="utf-8"))
    kept = {name: entry for name, entry in logins.items()
            if isinstance(entry, dict) and (entry.get("type") == "api_key"
                                            or (entry.get("expires", 0) / 1000 - time.time()) / 60 >= 40)}
    del logins
    for gone in ("openai-codex/gpt-gone-9", "anthropic/claude-gone-9"):
        provider = gone.split("/", 1)[0]
        print("\n== pi on", gone)
        if provider not in kept:
            print("no login of", provider, "with 40 minutes of life: not run")
            continue
        scratch = tempfile.mkdtemp(prefix=".cf-gone-launch-", dir=HOME)
        try:
            agent = os.path.join(scratch, "home", ".pi", "agent")
            work = os.path.join(scratch, "work")
            os.makedirs(agent)
            os.makedirs(work)
            copy = os.path.join(agent, "auth.json")
            with open(os.open(copy, os.O_WRONLY | os.O_CREAT, 0o600), "w") as fh:
                json.dump({provider: kept[provider]}, fh)
            print("login copied:", provider, "set")
            with open(os.path.join(agent, "settings.json"), "w") as fh:
                fh.write("{}\n")
            env = clean_env(HOME=os.path.join(scratch, "home"),
                            CARGO_HOME=os.environ.get("CARGO_HOME", os.path.join(HOME, ".cargo")),
                            RUSTUP_HOME=os.environ.get("RUSTUP_HOME", os.path.join(HOME, ".rustup")))
            watch([shutil.which("pi"), "--no-extensions", "--no-mcp", "--model", gone], env, work)
            found = glob.glob(os.path.join(agent, "sessions", "**", "*.jsonl"), recursive=True)
            print("the session's record:", "none" if not found else os.path.basename(found[0]))
            for path in found[:1]:
                for line in open(path, encoding="utf-8"):
                    try:
                        entry = json.loads(line)
                    except ValueError:
                        continue
                    message = entry.get("message") or {}
                    if message.get("role") == "assistant":
                        print("   assistant:", message.get("provider"), message.get("model"),
                              message.get("stopReason"), (message.get("errorMessage") or "")[:300])
        finally:
            shutil.rmtree(scratch, ignore_errors=True)
            print("throwaway removed:", not os.path.exists(scratch))
    print("the owner's auth.json unchanged:", digest(source) == owner)


{"claude": claude, "codex": codex, "pi": pi}[sys.argv[1] if len(sys.argv) > 1 else "claude"]()
