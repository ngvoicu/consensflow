# Research — is Kimi Code a ConsensFlow harness? (live-probed 2026-08-24)

A harness needs five things. All five exist; every line below was run, not
remembered.

| Requirement | Kimi Code 0.38.0 |
|---|---|
| Skills directory | `$KIMI_CODE_HOME/skills/`, else `~/.kimi-code/skills/` — same Agent Skills `SKILL.md` format, auto-discovered at User scope |
| One-shot run | `kimi -p "<task>" --output-format stream-json` |
| Full permissions | **implied** in `-p` mode — no flag, and `--auto`/`--yolo` are REFUSED with `-p` |
| Session capture | last stream line: `{"role":"meta","type":"session.resume_hint","session_id":"session_…"}` |
| Resume (one-shot) | `kimi -p "<task>" -S <id>` — proven to carry context |
| Interactive window | `kimi -S <id>` (no `-p`) — **not verified**, cannot be run from a pipe |
| Session store | `~/.kimi-code/sessions/wd_<name>_<hash>/<sessionId>/agents/main/wire.jsonl` |
| Model | `-m <alias>`, aliases from `config.toml` |
| Effort | **no CLI flag** — `default_effort` per model in `config.toml` |

## The probes

```
$ kimi -p "…" --output-format stream-json --auto
error: Cannot combine --prompt with --auto.          # ditto --yolo

$ kimi -p "…" --output-format stream-json -S cf-probe-one
error: failed to run prompt: Session "cf-probe-one" not found.
```

**`-S` only resumes; it cannot mint.** That makes kimi the codex shape, not the
pi shape: turn one streams and hands its id back, and only then can a window
open on it.

```
$ kimi -p "Reply with exactly: pong" --output-format stream-json
{"role":"meta","type":"system.version","version":"0.38.0"}
{"role":"assistant","content":"pong"}
{"role":"meta","type":"session.resume_hint","session_id":"session_ba23…"}

$ kimi -p "What did I just ask you to reply?" -S session_ba23… --output-format stream-json
{"role":"assistant","content":"You asked me to reply with exactly: `pong`"}
```

The id arrives at the END of the stream, not the start as codex's does. Our
capture reads every line, so this costs nothing — but it means a run killed
early loses the session.

## The session store

`wire.jsonl` is an event log, not a message list:

- user turns — `{"type":"turn.prompt","input":[{"type":"text","text":…}],"origin":{"kind":"user"}}`
- assistant turns — `{"type":"context.append_loop_event","event":{"type":"content.part","turnId":"0","part":{"type":"text","text":…}}}`, with `part.type: "think"` for reasoning
- `context.append_message` also carries user text, but wrapped with the
  environment's own `<system-reminder>` blocks — `turn.prompt` is the clean source

**Parts must be joined per `turnId`.** A one-word answer is one part; a real
answer is many, and one turn per part would inflate the count that `--unread`
and `--wait` both key on.

`session_index.jsonl` maps sessionId → sessionDir, but it is a cache beside the
store, not the store: read the sessions tree, like every other harness.

## What this costs

Two things are unlike the other four:

1. **No per-run effort.** A kimi agent carries a model and nothing else — the
   effort lives in the user's own `config.toml`, which is theirs to edit and
   not ours to write (their API key is in that file).
2. **No billing guard.** claude and codex have an env var whose presence
   silently switches billing; kimi authenticates from `config.toml`, so there
   is nothing to strip. `dropEnv` is empty on purpose, not by omission.
