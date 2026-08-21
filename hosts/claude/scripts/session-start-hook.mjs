#!/usr/bin/env node
// SessionStart hook: stash the live session's transcript path so cf.mjs can build handoffs
// (Bash subprocesses get no session env from the host), and surface a short ConsensFlow
// availability note as context. Must never block session start — always exits 0.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { driftedAgents } from "../../lib/presets.js";
import { ensureCfDirs, loadAgents, saveSession } from "../../lib/state.js";
import { readStdinText } from "./hook-io.mjs";

const CLI_PATH = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "bin", "cf.mjs");

try {
  // Inside an agent subprocess (claude-code child): never touch the lead session's stash.
  if (process.env.CONSENSFLOW_CHILD) process.exit(0);
  const input = JSON.parse((await readStdinText()) || "{}");
  const cwd = input.cwd || process.cwd();
  await ensureCfDirs(cwd);
  await saveSession(cwd, { sessionId: input.session_id, transcriptPath: input.transcript_path, source: input.source });

  const agents = await loadAgents(cwd).catch(() => []);
  const roster = agents.length
    ? agents.map((p) => `@${p.id} (${p.kind}${p.model ? ` ${p.model}` : ""})`).join(", ")
    : "none configured yet — `agents add <preset>` (see `agents presets`)";
  console.log(
    [
      "ConsensFlow is available: consult one named AI agent (an external coding-harness CLI, run one-shot with a session handoff) for advice, second opinions, implementation help, or write-capable task execution.",
      `CLI: node "${CLI_PATH}" — subcommands: status | doctor | agents … | run @name <prompt>`,
      `Agents: ${roster}${driftNote(agents)}`,
      "Consulting is free and encouraged (one at a time). Acting is gated: never apply an agent's advice or file changes without the user's approval, unless they pre-authorized it.",
    ].join("\n"),
  );
} catch {
  // A broken hook must never break the session.
}
process.exit(0);

// A ConsensFlow update ships a new preset catalog, but roster entries keep the models they were
// added with until synced. Say so once, where the lead sees it at session start.
function driftNote(agents) {
  const drifted = driftedAgents(agents);
  if (drifted.length === 0) return "";
  return `\nCatalog: ${drifted.length} agent${drifted.length === 1 ? " is" : "s are"} behind the current preset catalog — run \`agents sync\` to upgrade their models.`;
}
