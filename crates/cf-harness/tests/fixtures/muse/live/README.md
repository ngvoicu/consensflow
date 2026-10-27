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

`background-outlives-turn.jsonl` is the log of the live case
`crates/cf-e2e/tests/live_muse_background.rs` (T-789, 2026-10-27 07:38): Muse
1.4.13-R6996.1 on `muse-spark-1.3` at `minimal`, contributor mode off, `--yolo`, in a
throwaway home, scrubbed by `cargo xtask scrub-muse-session`. The model started a
40-second command in the background (55, its task's output `background_running` at
65), answered and ended its turn (80, 86). The command's end was queued (90) and its
task completed (92); Muse started the run it wakes (95, no message of the user's) and
then drained the end into it (107); that run answered (119) and ended (125), and the
window quit (126).

T-801 (Phoebus, 2026-10-27) recorded six shapes on Muse 1.4.13-R6996.1,
`muse-spark-1.3` at `minimal`, contributor mode off, `--yolo`, each in a throwaway home
and project, scrubbed by `cargo xtask scrub-muse-session` and then T-705's identity
scrubber (paths, uuids, call ids, sockets as placeholders). Copied unchanged from
`consensflow-trees/.runs/muse-research/fixtures/t-shapes/<case>/session.jsonl`, where
each case's README, argv, actions, usage, hooks and screens are. The line numbers below
are physical lines, as `src/muse/record/tests/shapes.rs` reads them.

- `monitor-outlives-turn.jsonl`: a monitor armed (75, `work.v2.monitor.`); the turn ends
  (95); its two events are queued under the ended run (98, 99) and its task completes
  (100); another run starts (103), drains both (115, 116) and ends (136).
- `child-not-waited.jsonl`: a child spawned (67) and running (77); the parent's turn
  ends (104) without `subagent_wait`; the child's `result_ready` (107) and its result
  queued under the old run (108); a new run starts (111), drains it (123), ends (143).
- `stopped-by-the-model.jsonl`: a background command (74) the model stops with
  `work_stop` (91, accepted 109): its end queued (120, `terminal_kind: cancelled`), its
  task cancelled (122, `terminated by bash_input`), a delivery anomaly (123); the turn
  ends (134); a new run (138) drains it (150) and ends (170).
- `stopped-by-the-human.jsonl`: the same stopped from `/tasks` after the turn ended
  (95): queued (99), cancelled (101, `cancelled by runtime client`), a new run (104)
  drains it (116), ends (136).
- `killed-and-resumed.jsonl`: Muse killed (SIGKILL) after its turn ended (94) with a
  background command running; its first 95 lines are the log as the kill left it. The
  resume writes `session.end` `crash_inferred` (96), starts with `background_tasks: keep`
  (98–100), `session.resumed` (104), and cancels the old task itself (106,
  `resume_reconcile:orphaned_by_process_loss`); no delivery, no run.
- `goal.jsonl`: `/goal` queued as `goal_control` `set` (24) before any run; drained into
  a run (42) and bound to it (43, `goal_run_binding`); `update_goal` `complete` (129,
  its result 145); the run ends (165).
- `escape-before-a-word.jsonl`: Escape right after the run started: `terminal
  cancelled` (46) and `run_retracted` `no_output_interrupt_restore` (47), no word of
  the model's.
