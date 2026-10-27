# Muse Code session logs of a real model: provenance

`contributor-full-run.jsonl` is the parent session's whole log of T-705's live run
(MUS-02, 2026-10-26), copied unchanged from
`consensflow-trees/.runs/muse-research/fixtures/contributor-full-run/session.jsonl`,
where its README, the per-case manifests and the run's ledger are. Muse Code
1.4.13-R6996.1 in its TUI, in a pty, on a synthetic throwaway project, launched
`--model muse-spark-1.3-contributor --reasoning-effort minimal` (contributor mode on,
allowed for tests by Gabriel, 2026-10-26 18:54; Meta may train on what such a model
sees, so nothing in it is the owner's), with an isolated config and data folder. The
tester scrubbed it structurally: paths, UUIDs, opaque ids, pids and long context
become consistent placeholders (`00000000-0000-4000-8000-…`, `<PROJECT>`,
`<FIXTURE_ROOT>`, `<SCRUBBED_LONG_VALUE>`); retained-frame digests no longer match.

What it holds, by record kind (sequence numbers are the log's own):

- a plain turn (22–97); a tool turn, one bash that succeeds and one that exits 7
  (104–252);
- a monitor and a child agent, started together (294): the monitor's task 528 runs
  until its stream ends (352), its two `monitor_event` items queued (350, 351) and
  drained (381, 382); the child (`subagent.control`, 319–354) delivers its result
  (`subagent_result`, 355, drained 373) to `subagent_wait`; the turn ends at 441;
- a resume (446–450), then a background command (525): its task 1027 says
  `execution_state: background_running` (537), the call's result comes at once
  (541), the command ends (`background_task_terminal` queued 553, task completed
  555), and the item is drained into the same run (582), which ends at 602;
- a question (`request_user_input`: 643, 644);
- an interrupt after output: a foreground bash cancelled with its turn (719, 723);
- `/compact` after it (724–747, the summary at 742's `replacement.text`);
- a resume, an approval in `--approval-mode untrusted` (802–810), and `/clear`
  (847–849).

In each, the background work ends inside the turn that started it, because the agent
waited on it (`bash_input`, `subagent_wait`). Work that outlives its turn is the live
case `crates/cf-e2e/tests/live_muse_background.rs`.
