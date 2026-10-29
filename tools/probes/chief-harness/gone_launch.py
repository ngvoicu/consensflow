#!/usr/bin/env python3 -I
"""Claude Code's window launched on a model its account does not have (CHF-06, the
gone-model launch after Calliope's T-955 P1): does its record show anything before it
ends? A chief's window counts as come up where its record shows anything of it (`named`),
and only a launch that ends with a code of failure before that is read in its screen's
words, so the answer says whether the gone model of a real Claude launch is heard.

    python3 -I tools/probes/chief-harness/gone_launch.py

One window, in a throwaway CLAUDE_CONFIG_DIR seeded from the Keychain (its login said
"set"), every CONSENSFLOW_* variable stripped, no model turn spent (the model is refused).
It prints how the window ended, its screen's last lines, and every file the throwaway's
`projects/` holds for the session, with the types of the entries in it.
"""
import glob, json, os, shutil, subprocess, sys, tempfile, time, uuid

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tuidrive  # noqa: E402

HOME = os.path.expanduser("~")
GONE = "claude-gone-9-9"
DROPPED = ("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_")


def main():
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
        env = {k: v for k, v in os.environ.items() if not k.upper().startswith(DROPPED)}
        env["CLAUDE_CONFIG_DIR"] = config
        session = str(uuid.uuid4())
        argv = [shutil.which("claude"), "--strict-mcp-config", "--no-chrome", "--model", GONE,
                "--session-id", session]
        drive = tuidrive.Session(argv, env, work, limit=180)
        try:
            start = time.time()
            while drive.alive() and time.time() - start < 60:
                drive.pump(0.5)
            alive = drive.alive()
            print("the window ended by itself within 60 s:", not alive)
            print("its exit code:", None if alive else drive.proc.returncode)
            print("its screen's last lines:")
            for line in [line for line in drive.text() if line.strip()][-6:]:
                print("   |", line.rstrip()[:150])
            if alive:
                # Still up: the refusal comes at the first turn, as Codex's and Muse's do.
                drive.type("Reply with exactly: ONE", enter=True)
                drive.wait_quiet(quiet=4, maxwait=90)
                print("after one turn, alive:", drive.alive())
                for line in [line for line in drive.text() if line.strip()][-6:]:
                    print("   |", line.rstrip()[:150])
        finally:
            drive.kill()
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


main()
