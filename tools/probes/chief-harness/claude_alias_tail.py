#!/usr/bin/env python3 -I
"""Claude Code's alias pinning, and what a tail read of a long transcript costs (CHF-02).

    python3 -I tools/probes/chief-harness/claude_alias_tail.py [alias|tail|both]
        [--in FOLDER] [--megabytes 120]

alias: a conversation started with `--model sonnet` (an alias), in a throwaway
  CLAUDE_CONFIG_DIR seeded from the Keychain (a file on Windows), one short turn; then
  resumed with no flag. Prints what its record says of the model at each step
  (`message.model`, `requestedModel`): whether the record pins the id the alias resolved
  to, so that a reopen that passes that id stays on it when the alias moves.

tail: a transcript of `--megabytes` MB written in the throwaway, in the shape of Claude's
  (user and assistant entries, tool results of up to 2 MB, sidechain entries), with its
  last real assistant entry at a known place; then the time and bytes of (a) a full read
  that parses every line and (b) a tail read that reads 64 KB blocks from the end until a
  non-sidechain, non-synthetic assistant entry is whole, parsing only the lines that name
  an assistant, stopping at 16 MB. Run twice:
  with the entry near the end, and with a 30 MB tool output after it (past the bound).

Nothing is read of the owner's own transcripts; the turn's text is never printed.
"""
import getpass, glob, json, os, random, shutil, subprocess, sys, tempfile, time, uuid

WINDOWS = sys.platform == "win32"
WHAT = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else "both"


def flag(name, default):
    return sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default


BLOCK = 64 * 1024
BOUND = 16 << 20
DROPPED = ("CONSENSFLOW_", "CLAUDE_CODE_", "ANTHROPIC_")


def remove(path):
    for _ in range(10):
        shutil.rmtree(path, ignore_errors=True)
        if not os.path.exists(path):
            return
        time.sleep(0.5)


def model_of(entry):
    """What a CHF-05 reader takes of an entry: the model of a real assistant turn, or none."""
    if entry.get("type") != "assistant" or entry.get("isSidechain"):
        return None
    model = (entry.get("message") or {}).get("model")
    if not model or model == "<synthetic>":
        return None
    return {"model": model, "effort": entry.get("effort"), "requestedModel": entry.get("requestedModel")}


def alias(scratch):
    config = os.path.join(scratch, "claude")
    os.makedirs(config)
    if WINDOWS:
        raw = open(os.path.join(os.path.expanduser("~"), ".claude", ".credentials.json")).read()
    else:
        raw = subprocess.run(["security", "find-generic-password", "-s", "Claude Code-credentials",
                              "-a", getpass.getuser(), "-w"], capture_output=True, text=True,
                             timeout=20).stdout.strip()
    oauth = json.loads(raw)["claudeAiOauth"]
    if (oauth["expiresAt"] / 1000 - time.time()) / 3600 < 2:
        sys.exit("refusing: the Claude login expires within 2 h")
    with open(os.open(os.path.join(config, ".credentials.json"), os.O_WRONLY | os.O_CREAT, 0o600), "w") as fh:
        json.dump({"claudeAiOauth": oauth}, fh)
    print("login copied: claudeAiOauth set")
    del raw, oauth
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith(DROPPED)}
    env["CLAUDE_CONFIG_DIR"] = config
    work = os.path.join(scratch, "work")
    os.makedirs(work)
    session = str(uuid.uuid4())
    base = [shutil.which("claude"), "-p", "--strict-mcp-config", "--no-chrome", "--output-format", "json"]
    for name, extra in [("start --model sonnet", ["--model", "sonnet", "--session-id", session]),
                        ("resume, no flag", ["--resume", session]),
                        ("resume --model opus (alias)", ["--resume", session, "--model", "opus"])]:
        done = subprocess.run(base + extra + ["Reply with exactly: CF_OK"], env=env, cwd=work, input="",
                              capture_output=True, text=True, timeout=240)
        found = glob.glob(os.path.join(config, "projects", "*", session + ".jsonl"))
        last = None
        for line in open(found[0], encoding="utf-8") if found else []:
            try:
                last = model_of(json.loads(line)) or last
            except ValueError:
                pass
        print("  %-30s exit %s  record %s" % (name, done.returncode, json.dumps(last)))


def write_transcript(path, megabytes, after_mb):
    """A transcript of about `megabytes` MB whose last real assistant entry is followed by
    `after_mb` MB of tool output and sidechain entries."""
    rng = random.Random(7)
    session = str(uuid.uuid4())
    filler = "x" * 1024

    def entry(kind, size, **more):
        return json.dumps(dict({"type": kind, "sessionId": session, "uuid": str(uuid.uuid4()),
                                "message": {"role": kind, "content": filler * size}}, **more))

    written = 0
    with open(path, "w") as out:
        while written < (megabytes - after_mb) << 20:
            for line in (entry("user", rng.randint(1, 64)),
                         entry("assistant", rng.randint(1, 8), effort="high",
                               message={"role": "assistant", "model": "claude-haiku-5-5", "content": "ok"}),
                         entry("user", rng.randint(1, 2048))):
                out.write(line + "\n")
                written += len(line) + 1
        last = json.dumps({"type": "assistant", "effort": "low", "requestedModel": "claude-sonnet-5-5",
                           "message": {"role": "assistant", "model": "claude-sonnet-5-5", "content": "the last"}})
        out.write(last + "\n")
        written += len(last) + 1
        while written < megabytes << 20:
            for line in (entry("user", 2048), entry("assistant", 4, isSidechain=True,
                                                    message={"role": "assistant", "model": "claude-haiku-5-5"}),
                         json.dumps({"type": "assistant", "message": {"role": "assistant", "model": "<synthetic>"}})):
                out.write(line + "\n")
                written += len(line) + 1
    return written


def full_read(path):
    last, read = None, 0
    with open(path, "rb") as fh:
        for line in fh:
            read += len(line)
            try:
                last = model_of(json.loads(line)) or last
            except ValueError:
                pass
    return last, read


def tail_read(path):
    size = os.path.getsize(path)
    read, end, carry = 0, size, []
    with open(path, "rb") as fh:
        while end > 0 and read < BOUND:
            start = max(0, end - BLOCK)
            fh.seek(start)
            block = fh.read(end - start)
            read += end - start
            end = start
            if b"\n" not in block and start > 0:
                # Inside one long line: its pieces are kept, and joined once it is whole.
                carry.append(block)
                continue
            lines = (block + b"".join(reversed(carry))).split(b"\n")
            # The first piece may be a line cut by the block's start: kept for the next block.
            carry = [lines[0]] if start > 0 else []
            whole = lines[1:] if start > 0 else lines
            for line in reversed(whole):
                # A line that names no assistant is never parsed: a tool's output can be
                # megabytes of JSON a reader has no use for.
                if b'"assistant"' not in line:
                    continue
                try:
                    found = model_of(json.loads(line))
                except ValueError:
                    continue
                if found:
                    return found, read
    return None, read


def tail(scratch):
    megabytes = int(flag("--megabytes", "120"))
    for after in (1, 30):
        path = os.path.join(scratch, "transcript-%d.jsonl" % after)
        size = write_transcript(path, megabytes, after)
        started = time.perf_counter()
        whole, whole_bytes = full_read(path)
        full_ms = (time.perf_counter() - started) * 1000
        started = time.perf_counter()
        found, tail_bytes = tail_read(path)
        tail_ms = (time.perf_counter() - started) * 1000
        print("  %d MB, last entry %d MB from the end: full read %.0f ms over %.1f MB -> %s; "
              "tail read %.1f ms over %.2f MB -> %s" % (
                  size >> 20, after, full_ms, whole_bytes / 2**20, whole and whole["model"],
                  tail_ms, tail_bytes / 2**20, found and found["model"]))
        os.remove(path)


def main():
    scratch = tempfile.mkdtemp(prefix="cf-claude-alias-tail-", dir=flag("--in", None))
    try:
        if WHAT in ("alias", "both"):
            alias(scratch)
        if WHAT in ("tail", "both"):
            tail(scratch)
    finally:
        remove(scratch)
    print("throwaway removed:", not os.path.exists(scratch))


main()
