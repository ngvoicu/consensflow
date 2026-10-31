#!/usr/bin/env python3 -I
"""What Claude Code's record keeps of a `/model` the human opens and cancels, beside one they
confirm for the conversation (CHF-05, Calliope's P3 of T-994): the walk back of `last_used`
stops where the human chose the newest model, and must not stop at a choice never made.

    python3 -I tools/probes/chief-harness/model_cancel.py

One window on Haiku, in a throwaway CLAUDE_CONFIG_DIR seeded from the Keychain (its login said
"set", refused under 2 h of life), every CONSENSFLOW_* variable stripped, its workspace under
the real home. One turn, then `/model` cancelled with Escape, then `/model` confirmed on Sonnet
with `s` (this conversation only) and a second turn, then the window quit. It prints every user and system entry
of the session's record that is a command's echo or its output, in order, and whether the
throwaway's settings.json and the owner's ~/.claude/settings.json are unchanged.
"""
import glob, hashlib, json, os, re, shutil, subprocess, sys, tempfile, time, uuid

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tuidrive  # noqa: E402

HOME = os.path.expanduser("~")
DROPPED = ("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_")
OWNER = os.path.join(HOME, ".claude", "settings.json")


def digest(path):
    try:
        with open(path, "rb") as fh:
            return hashlib.sha256(fh.read()).hexdigest()[:12]
    except OSError:
        return None


def picker(drive, title):
    drive.type("/model", enter=True, settle=2)
    drive.wait_quiet(quiet=2, maxwait=30)
    shown = [row.rstrip() for row in drive.text() if re.search(r"\d+\.", row)]
    print(title, "picker rows:", len(shown))


def main():
    before = digest(OWNER)
    scratch = tempfile.mkdtemp(prefix=".cf-model-cancel-", dir=HOME)
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
        if (oauth["expiresAt"] / 1000 - time.time()) / 3600 < 2:
            sys.exit("refusing: the Claude login expires within 2 h")
        with open(os.open(os.path.join(config, ".credentials.json"), os.O_WRONLY | os.O_CREAT, 0o600), "w") as fh:
            json.dump({"claudeAiOauth": oauth}, fh)
        del raw, oauth
        print("login copied: claudeAiOauth set")
        with open(os.path.join(config, ".claude.json"), "w") as fh:
            json.dump({"hasCompletedOnboarding": True, "theme": "dark",
                       "projects": {work: {"hasTrustDialogAccepted": True}}}, fh)
        settings = os.path.join(config, "settings.json")
        with open(settings, "w") as fh:
            fh.write("{}\n")
        env = {k: v for k, v in os.environ.items() if not k.upper().startswith(DROPPED)}
        env["CLAUDE_CONFIG_DIR"] = config
        session = str(uuid.uuid4())
        argv = [shutil.which("claude"), "--strict-mcp-config", "--no-chrome", "--model",
                "claude-haiku-5-5", "--session-id", session]
        drive = tuidrive.Session(argv, env, work, limit=600)
        try:
            drive.wait_quiet(quiet=4, maxwait=90)
            drive.type("Reply with exactly: ONE", enter=True)
            drive.wait_quiet(quiet=5, maxwait=120)
            picker(drive, "1. /model opened;")
            drive.key("esc", settle=2)
            drive.wait_quiet(quiet=2, maxwait=20)
            print("   after Escape, last lines:")
            for line in [line for line in drive.text() if line.strip()][-4:]:
                print("   |", line.rstrip()[:150])
            picker(drive, "2. /model opened again;")
            for _ in range(20):
                at = [row for row in drive.text() if re.search(r"❯ \d+\.", row)]
                if at and "sonnet" in at[-1].lower():
                    break
                drive.key("up", settle=0.5)
            drive.send(b"s")
            # A conversation with a turn asks first: its cache is the old model's.
            drive.wait_quiet(quiet=2, maxwait=20)
            if any("Yes, switch to" in row for row in drive.text()):
                drive.key("enter", settle=2)
            drive.wait_quiet(quiet=2, maxwait=20)
            # A turn on the model chosen, so that the record shows it after the switch.
            drive.type("Reply with exactly: TWO", enter=True)
            drive.wait_quiet(quiet=5, maxwait=120)
            print("   after s, last lines:")
            for line in [line for line in drive.text() if line.strip()][-4:]:
                print("   |", line.rstrip()[:150])
            drive.key("ctrl-c", settle=0.5)
            drive.key("ctrl-c", settle=2)
        finally:
            drive.kill()
        found = glob.glob(os.path.join(config, "projects", "*", session + ".jsonl"))
        print("3. the record's command entries, in order:")
        for path in found:
            for line in open(path, encoding="utf-8"):
                try:
                    entry = json.loads(line)
                except ValueError:
                    continue
                message = entry.get("message") or {}
                content = message.get("content") if isinstance(message.get("content"), str) else entry.get("content")
                if isinstance(content, str) and ("command-" in content or "local-command" in content):
                    print("   ", entry.get("type"), entry.get("subtype") or "", content[:200].replace("\n", " "))
                elif entry.get("type") == "assistant":
                    print("    assistant", message.get("model"))
        with open(settings) as fh:
            print("4. the throwaway's settings.json unchanged:", fh.read() == "{}\n")
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
        print("   the owner's settings.json unchanged:", digest(OWNER) == before)
        print("   throwaway removed:", not os.path.exists(scratch))


main()
