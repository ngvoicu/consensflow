# Research — what each CLI can do interactively (live-verified 2026-08-24)

The question: can a consult BE the harness's own window from its very first
turn, on a session we can later resume and read?

| Harness | Fresh window on a KNOWN id | Seed a first message | Resume a window |
|---|---|---|---|
| claude | `claude --session-id <uuid>` — we mint the uuid | positional `[prompt]` | `claude --resume <id> [prompt]` |
| pi | `--session-id <id>` "creating it if missing" — we mint the name | positional `[messages...]` | same flag, same id |
| opencode | **no** — the TUI creates its own `ses_…` id | `--prompt <text>` | `opencode --session <id>` |
| codex | **no** — interactive `codex` takes no pre-set id, and none is printed | positional `[PROMPT]` | `codex resume <id> [PROMPT]` |

So two harnesses take an id from us, and two cannot:

- **opencode**: the id is discoverable. `~/.local/share/opencode/storage/
  session/<projectID>/<ses_id>.json` carries `directory` (the cwd) and
  `time.created` — poll after spawning for a session in this directory created
  after the spawn. Verified against the real store.
- **codex**: not even discoverable reliably before the TUI writes its rollout,
  and `codex resume` needs the id. But our one-shot `codex exec --json` already
  captures `thread_id` on turn 1 — so codex streams its first answer, then the
  pane becomes `codex resume <id>`. The user chose this trade knowingly.

Also verified:

- `cf catchup jade-waves` reads a live pi conversation by our minted id and
  unwraps the packet to the question — the read path for window turns works
  end-to-end today.
- `interactiveResume` covers all four kinds already.
- **Billing-guard gap**: `handOver` spawns the interactive window with the full
  inherited environment — no `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` strip. Every
  attached turn routes around the guard the one-shot runs carry. Must fix.
- opencode has no `session create` — only list/delete. Discovery it is.
