#!/usr/bin/env python3
"""What Codex's own key reader (crossterm, the fork Codex 0.167.0 pins) reads of the bytes a pane
writes for a Shift+Enter, through a real pty on this Mac (T-1336).

    python3 -I tools/probes/key-reader/run.py            # prints a JSON line per case
    python3 -I tools/probes/key-reader/run.py --record   # and writes results.json beside it

Each case fills the pty as a busy window's input would be (a bracketed paste, or typed characters up
to the queue's end), writes the key in ONE write, and lets the reader, which waits 200 ms, read:
the key's bytes are read whole where the queue has room for both, and in two reads where an Escape
is the last byte it takes. Needs cargo and network on the first run (the fork is a git dependency).
"""
import json, os, pty, select, subprocess, sys, time
from pathlib import Path

HERE = Path(__file__).resolve().parent
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", HERE.parents[2] / "app/src-tauri/target/probes/key-reader"))
BIN = TARGET / "debug/key-reader-probe"
ESC_CR, LF = b"\x1b\r", b"\n"
CSI_U = b"\x1b[13;2u"
MODIFY_OTHER_KEYS = b"\x1b[27;2;13~"
PASTE = b"\x1b[200~" + b"x" * 1009 + b"\x1b[201~"
CASES = [
    ("nothing waiting, CR", b"", b"\r"),
    ("nothing waiting, ESC", b"", b"\x1b"),
    ("nothing waiting, ESC CR", b"", ESC_CR),
    ("nothing waiting, LF", b"", LF),
    ("nothing waiting, CSI u", b"", CSI_U),
    ("nothing waiting, modifyOtherKeys", b"", MODIFY_OTHER_KEYS),
    ("1020 typed characters, ESC CR", b"x" * 1020, ESC_CR),
    ("1021 typed characters, ESC CR", b"x" * 1021, ESC_CR),
    ("1021 typed characters, LF", b"x" * 1021, LF),
    ("a 1,009-character bracketed paste, ESC CR", PASTE, ESC_CR),
    ("a 1,009-character bracketed paste, LF", PASTE, LF),
]


def build():
    subprocess.run(
        ["cargo", "build", "--manifest-path", str(HERE / "Cargo.toml")],
        check=True, env={**os.environ, "CARGO_TARGET_DIR": str(TARGET)},
    )


def run(name, prefix, key):
    master, slave = pty.openpty()
    child = subprocess.Popen([str(BIN)], stdin=slave, stdout=slave, stderr=slave)
    os.close(slave)
    seen, deadline = b"", time.monotonic() + 5
    while b"READY" not in seen and time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            seen += os.read(master, 65536)
    assert b"READY" in seen, (name, seen)
    if prefix:
        os.write(master, prefix)
    started = time.monotonic()
    written = os.write(master, key)
    wrote_ms = (time.monotonic() - started) * 1000
    while child.poll() is None and time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                break
            if not chunk:
                break
            seen += chunk
    child.wait(timeout=2)
    os.close(master)
    result = next(line for line in seen.decode().splitlines() if line.startswith("RESULT"))
    return {"case": name, "key": list(key), "key_write_bytes": written,
            "key_write_ms": round(wrote_ms, 1), "result": result.strip()}


if __name__ == "__main__":
    build()
    rows = [run(*case) for case in CASES]
    for row in rows:
        print(json.dumps(row), flush=True)
    if "--record" in sys.argv:
        (HERE / "results.json").write_text(json.dumps(
            {"crossterm": "openai-oss-forks/crossterm ed1cdab335221515706178d68495bba2aed1924f (Codex 0.167.0)",
             "system": "macOS pty, raw mode, the reader waits 200 ms", "cases": rows}, indent=2) + "\n")
