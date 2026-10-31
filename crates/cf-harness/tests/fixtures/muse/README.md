# Muse Code session logs of real runs: provenance

Recorded 2026-10-26 (T-703) by `record.py` beside this file, on Muse Code 1.4.13
(`muse-bin-1.4.13-R6996.1`, run directly, never through its launcher, with
`MUSE_NO_AUTO_UPDATE=1`) and its built-in fake model (`--provider echo
--echo-delay-ms 6000`: offline, deterministic, no login, no quota), in a pty
110×40, in a throwaway home with its own XDG folders and a git workspace
(`--trust-workspace`). Nothing of the owner's Muse or home was read or written:
the owner's `~/.config/muse` and `~/.local/bin/.muse-*` were hashed before and
after. Each log was then made a fixture by `cargo xtask scrub-muse-session
<session.jsonl> <fixture.jsonl>`: every record stays, in its order, with every key;
the throwaway home becomes `/muse-home` and the temporary folder `/tmp/`, and
Muse's own words (its base prompt, the context it builds for a request, its
diagnostics of that context, its own context blocks) become `[scrubbed: <n>
bytes]`. The user's words, the model's (`echo: <the prompt>`), a hook's context,
and every id, kind and time are as Muse wrote them.

The cases (`python3 -I record.py <muse-bin> <out folder> [case...]`):

- `plain.jsonl`: two turns, each answered, then two Ctrl-Cs, a clean exit
  (`session.end`, `exit_reason: clean`).
- `retracted.jsonl`: Escape 1.5 s into a turn, before a word of it: `terminal
  cancelled` (`cancelled during model step`) then `run_retracted`
  (`no_output_interrupt_restore`), the message back in the box; Ctrl-C clears
  it, a second turn is answered, a clean exit.
- `cut.jsonl`: one turn answered, a second at work when the process is killed
  (SIGKILL): no `terminal` of it, no `session.end`.
- `at-rest-killed.jsonl`: one turn answered, the process killed at rest: no
  `session.end`, every turn ended. The control of `cut`.
- `hooks.jsonl`: `plain` with hooks given for the run (`-c hooks=…`): a
  `SessionStart` hook whose stdout goes into the model's context
  (`context_block_updated`, `source: runtime_hook`) and a `UserPromptSubmit`
  hook that says nothing (`hook_run_started`, `hook_run_terminal`). Muse's own
  `session_identity` block comes with it.
- `cut-resumed.jsonl`: `cut`, then `muse resume --last` and a clean exit: the
  resume writes a `session.end` for the dead process (`crash_inferred`,
  `uptime_ms: 0`), `session.resumed`, and ends the cut turn itself (`terminal
  cancelled`, `resume_reconcile:orphaned_by_process_loss`).
- `quit-mid-turn.jsonl`: four Ctrl-Cs 2 s into a second turn. The first
  interrupts it (cancelled and taken back, as Escape does), the second clears
  the box, the last two quit: the turn's end comes before the process's.
- `steered.jsonl`: a message pasted 1.5 s into a turn: accepted at once
  (`runtime.user_intent.accepted`, `wake_policy: do_not_wake`, then
  `inbox_item_queued` from `user_steer`), and answered by a run of its own after
  the first, which carries its intent's id and no prompt.
- `index-states.json`: not a log. The rows of `session-index.db` (session,
  `prompt_count`, `latest_segment_terminated`) and the sessions whose logs are
  on disk, after each step of three sessions and a fourth window (answered and
  open, quit cleanly, a later window up, killed at rest, resumed, resumed and
  quit, a window quit before its first message).
- `session-index.schema.sql`: the index's schema as Muse 1.4.13 made it,
  `sqlite_master` in its order, and its `schema_meta` (a path and a fingerprint
  replaced).

What a turn with tools, questions, approvals, compaction, background work or a
refusal for quota writes is not here: the Mac's Muse login runs no model turn
(HTTP 402, T-700). The reader reads those from Muse's schema, marked provisional
(`src/muse/record/provisional.rs`), until MUS-02's live fixtures.

## Recorded by T-705 and T-718 on a Muse that runs turns (2026-10-26), kept for MUS-23, MUS-24

Copied byte for byte from the scrubbed fixtures the testers kept
(`consensflow-trees/.runs/muse-research/fixtures/`; their paths, ids and Meta's own text are
placeholders already), after the account paid for Muse (18:57):

- `billing-spark.jsonl`, `billing-wonder.jsonl` (T-705, `billing-spark-tui`,
  `billing-wonder-tui`): one turn each, `muse-spark-1.3` and `muse-wonder-1.0` at `minimal`
  with `--contributor-mode off`, **refused with HTTP 402 `billing_error`**: the turn `failed`,
  nothing answered; then `runtime.model_reconfigure.completed` moves the model to
  `muse-spark-1.3-contributor` (`previous.source` `startup`, no command of the human's before
  it), and no turn followed.
- `clear-before.jsonl`, `clear-after.jsonl` (T-718, `t718/clear-model-change`): a `/clear`.
  The old session's metadata and configuration name `muse-spark-1.3-contributor`,
  `command.invoked` records `/clear`; the new session's `session_start.intake_v3`
  (`activation_source: clear`, `previous_session_stream`) and metadata name `muse-spark-1.3`,
  and **no `runtime.model_reconfigure.completed` is in either log**. `clear-before` is an
  excerpt of six records of the old log (its metadata, a configuration, the command, its end).
- `launch-model-restored.jsonl` (T-705, `question-hook-bypass`): the control. A session
  resumed with `--model muse-spark-1.3-contributor` on a launch that named it: Muse
  reconfigures from `muse-spark-1.3` (`previous.source` `replay`, the model the stored session
  held) to the contributor model the launch asked for. That is the launch's own, not a move.
- `msp-initialize.json`, `msp-model-list.json`, `msp-model-list-bundled.json`: the answers of one
  bounded `muse serve --provider meta --no-session-log` (2026-10-27, no model turn), read on a
  throwaway copy of the config and of the model catalog cache. `msp-model-list.json` is the
  account's list (`source: providerCatalog`, five models); `msp-model-list-bundled.json` what a
  host answers where it has no login or no cached catalog (`source: bundledCatalog`, one stale
  model). `museHome` is a placeholder.
- `usage-allowance.json`, `usage-no-allowance.json`: `muse usage --json` on the paid account
  (T-718, 2026-10-26) and on the account with no allowance (T-705): `result` only.

`usage/read` over a bare one-shot of `muse serve` answers `{}` (the schema: omitted while the
host has observed nothing), so the allowance ahead of a refusal comes from `muse usage --json`.
