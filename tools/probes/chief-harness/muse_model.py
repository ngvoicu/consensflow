#!/usr/bin/env python3
"""Muse Code's `/model` and its resume (CHF-02): which record field says the model a
session runs on after the human switches it in the window, and what a resume with no
flag comes back on. Mac only (a pty), run with a Python that has `pyte`:

    python3 -m venv <scratch>/venv && <scratch>/venv/bin/pip install pyte
    <scratch>/venv/bin/python tools/probes/chief-harness/muse_model.py --in <scratch>

1. `muse exec` starts a session on muse-spark-1.3, minimal effort, contributor mode off.
2. `muse resume <id>` in a pty (tuidrive.py, T-445's driver): `/model`, the picker's
   screen kept, the next model chosen with Enter, one turn.
3. `muse resume <id>` again with no flag: one turn.
4. `muse --model muse-gone-9 resume <id>`: one turn, the window's last lines kept.
After each step, every record line of the session's own log that names a model is
printed, by kind and field (never a turn's words), and the screens are written beside the
throwaway's log. Muse runs in a throwaway XDG config and data folder holding copies of
`auth.json` (its login is the Keychain's, which the copy does not carry: the run refuses
where a login is kept in the file) and of the model catalog. Its settings there start
empty, so an Enter that saves a default saves it in the throwaway, never the owner's.
"""
import glob, json, os, shutil, subprocess, sys, tempfile, time

sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tuidrive  # noqa: E402

HOME = os.path.expanduser("~")
PROMPT = "Reply with exactly: CF_OK"
MODEL = "muse-spark-1.3"


def flag(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


def model_lines(folder, seen):
    """The record lines under `folder` that name a model, not printed before."""
    found = []
    for path in sorted(glob.glob(os.path.join(folder, "**", "*.jsonl"), recursive=True)):
        for number, line in enumerate(open(path, encoding="utf-8", errors="replace")):
            key = (path, number)
            if key in seen or "model" not in line:
                continue
            seen.add(key)
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            if not isinstance(entry, dict):
                continue
            fields = {}

            def walk(value, at):
                if isinstance(value, dict):
                    for name, inner in value.items():
                        walk(inner, at + "." + name if at else name)
                elif isinstance(value, str) and ("model" in at.lower() or at.endswith("effort")):
                    fields[at] = value[:80]

            walk(entry, "")
            kind = entry.get("payload_type") or entry.get("record_type") or "?"
            if "fail" in line or "error" in kind:
                # How a turn failed: its kind, and its error's first words.
                said = json.dumps(entry.get("payload"))
                at = max(said.find("error"), 0)
                found.append("%s:%d %s %s" % (os.path.basename(path), number, kind, said[at:at + 220]))
                continue
            if "fail" in line or "error" in kind:
                # How a turn failed, by its kind and its error's kind and first words.
                said = json.dumps(entry.get("payload"))
                at = max(said.find("error"), 0)
                found.append("%s:%d %s %s" % (os.path.basename(path), number, kind, said[at:at + 220]))
            if fields:
                found.append("%s:%d %s %s" % (os.path.basename(path), number, kind, json.dumps(fields)))
    return found


def main():
    auth = json.load(open(os.path.join(HOME, ".config", "muse", "auth.json")))
    storages = [provider.get("storage") for provider in (auth.get("providers") or {}).values()]
    if not storages or any(storage != "keychain" for storage in storages):
        sys.exit("refusing: a Muse login is kept in its file, which a copy would carry")
    del auth
    scratch = tempfile.mkdtemp(prefix="cf-muse-model-", dir=flag("--in", None))
    config, data = os.path.join(scratch, "config"), os.path.join(scratch, "data")
    os.makedirs(os.path.join(config, "muse"))
    shutil.copy2(os.path.join(HOME, ".config", "muse", "auth.json"), os.path.join(config, "muse", "auth.json"))
    os.makedirs(os.path.join(data, "muse"))
    shutil.copytree(os.path.join(HOME, ".local", "share", "muse", "model-catalog"),
                    os.path.join(data, "muse", "model-catalog"))
    print("copied: auth.json (its login is the Keychain's) and the model catalog")
    work = os.path.join(scratch, "work")
    os.makedirs(work)
    env = tuidrive.clean_env({
        "XDG_CONFIG_HOME": config, "XDG_DATA_HOME": data,
        "XDG_CACHE_HOME": os.path.join(scratch, "cache"), "XDG_STATE_HOME": os.path.join(scratch, "state"),
        "MUSE_NO_AUTO_UPDATE": "1",
    }, HOME)
    seen = set()
    muse = shutil.which("muse")
    try:
        started = subprocess.run([muse, "exec", "--model", MODEL, "--reasoning-effort", "minimal",
                                  "--contributor-mode", "off", "--json", PROMPT], env=env, cwd=work,
                                 capture_output=True, text=True, timeout=240, input="")
        # The session is the one the index of the throwaway holds: its id, and the model and
        # effort the index says of it.
        def indexed():
            import sqlite3
            db = sqlite3.connect("file:%s?mode=ro" % os.path.join(data, "muse", "session-index.db"), uri=True)
            try:
                columns = [row[1] for row in db.execute("PRAGMA table_info(sessions)")]
                wanted = [c for c in ("session_id", "model_id", "provider_id") if c in columns]
                wanted += [c for c in columns if "effort" in c]
                return wanted, db.execute("SELECT %s FROM sessions" % ", ".join(wanted)).fetchall()
            finally:
                db.close()

        columns, rows = indexed()
        session = rows[0][0] if len(rows) == 1 else None
        print("   the index:", columns, [row[1:] for row in rows])
        print("1. exec: exit %s, session %s" % (started.returncode, "found" if session else "none"))
        for line in model_lines(data, seen):
            print("   ", line)
        if not session:
            print("   no session id in the events; event keys:",
                  sorted({k for l in started.stdout.splitlines()[:20] for k in (json.loads(l) if l.startswith("{") else {})}))
            return

        def window(step, switch, model=None):
            given = ["--model", model] if model else []
            drive = tuidrive.Session([muse, "--contributor-mode", "off"] + given + ["resume", session], env, work,
                                     limit=600, rawlog=os.path.join(scratch, "%s.raw" % step))
            try:
                drive.wait_quiet(quiet=4, maxwait=60)
                if any("Do you trust this workspace?" in line for line in drive.text()):
                    # The throwaway's own folder, trusted in the throwaway's own config.
                    drive.key("enter", settle=2)
                    drive.wait_quiet(quiet=3, maxwait=30)
                if switch:
                    drive.type("/model", enter=False, settle=1.5)
                    print("   /model typed, before Enter:")
                    for line in drive.text():
                        if line.strip():
                            print("     |", line[:140])
                    drive.key("enter", settle=2)
                    drive.wait_quiet(quiet=3, maxwait=30)
                    drive.snap(os.path.join(flag("--screens", scratch), "%s-picker.txt" % step), "the /model picker", with_history=False)
                    print("   the picker:")
                    for line in drive.text():
                        if line.strip():
                            print("     |", line[:140])
                    drive.key("down")
                    drive.key("enter", settle=2)
                    drive.wait_quiet(quiet=3, maxwait=30)
                    drive.snap(os.path.join(flag("--screens", scratch), "%s-after.txt" % step), "after Enter", with_history=False)
                    print("   after Enter:")
                    for line in drive.text():
                        if line.strip():
                            print("     |", line[:140])
                    # The effort picker that follows the model's: left as it stands.
                    drive.key("esc", settle=1.5)
                    drive.wait_quiet(quiet=2, maxwait=20)
                answers = lambda: sum(line.count("CF_OK") for line in drive.text(True))
                before = answers()
                drive.type(PROMPT, enter=True)
                start = time.time()
                while answers() <= before + 1 and time.time() - start < 120:
                    drive.pump(0.5)
                answered = answers() > before + 1
                drive.wait_quiet(quiet=4, maxwait=60)
                print("   the turn answered:", answered)
                if model:
                    print("   the window after the turn:")
                    for line in drive.text():
                        if line.strip():
                            print("     |", line[:140])
                drive.key("ctrl-c", settle=0.3)
                drive.key("ctrl-c", settle=2)
            finally:
                drive.kill()

        print("2. resume, /model, the next model, one turn")
        window("2-switch", True)
        for line in model_lines(data, seen):
            print("   ", line)
        print("   the index:", indexed())
        print("3. resume with no flag, one turn")
        window("3-resume", False)
        for line in model_lines(data, seen):
            print("   ", line)
        print("   the index:", indexed())
        print("4. resume with --model of a model Muse does not have, one turn")
        window("4-gone", False, model="muse-gone-9")
        for line in model_lines(data, seen):
            print("   ", line)
        settings = os.path.join(config, "muse", "settings.json")
        if os.path.exists(settings):
            saved = json.load(open(settings))
            print("throwaway settings.json after:", json.dumps({k: saved[k] for k in saved if "model" in k or "effort" in k}))
    finally:
        if "--keep" not in sys.argv:
            shutil.rmtree(scratch, ignore_errors=True)
        print("throwaway removed:", not os.path.exists(scratch))


main()
