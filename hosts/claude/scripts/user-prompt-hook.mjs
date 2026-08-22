#!/usr/bin/env node
// UserPromptSubmit hook: keep the session stash fresh, and nothing else.
//
// It used to route as well — a prompt naming one agent was rewritten into
// instructions for the lead — which made Claude Code behave unlike pi and
// unlike a cmux pane. All three now do the same thing: watch input, stash the
// conversation so `cf run` can attach it, and leave the turn to the lead.
// Must never block the prompt — always exits 0.
import { ensureCfDirs, saveSession } from "../../lib/state.js";
import { readStdinText } from "./hook-io.mjs";


try {
  // Inside an agent subprocess: never touch the lead session's stash, never route.
  if (process.env.CONSENSFLOW_CHILD) process.exit(0);
  const input = JSON.parse((await readStdinText()) || "{}");
  const cwd = input.cwd || process.cwd();
  await ensureCfDirs(cwd);
  await saveSession(cwd, { sessionId: input.session_id, transcriptPath: input.transcript_path });

  // Nothing is routed here. Watching input is for the stash above — the same
  // thing pi's extension does — so `cf run` spawns with the conversation
  // already attached. Deciding to consult an agent, and composing the run, is
  // the lead's job in every harness: it reads the skill and runs the command.
  // A hook that quietly rewrote the turn made Claude Code the odd one out.
  process.exit(0);
} catch {
  // A broken hook must never block the user's prompt.
}
process.exit(0);
