#!/usr/bin/env node
// ConsensFlow CC — the CLI the Claude Code lead drives via the Bash tool.
// Mirrors consensflow-pi's /consensflow:cf router: agents admin, doctor, status, and one-at-a-time runs.
import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { codexAuthPath, loadCodexAuth } from "../../lib/codex-auth.js";
import { renderImageRun, runImageAgent as runImageRun } from "../../lib/image-run.js";
import { driftedAgents, formatPresets, getPreset, listPresetIds, agentFromPreset } from "../../lib/presets.js";
import {
  cfRoot,
  configHome,
  ensureCfDirs,
  getAgent,
  loadCurrent,
  loadAgents,
  loadSession,
  agentsPath,
  recordLatestRun,
  removeAgent,
  runsRoot,
  syncAgentsWithPresets,
  upsertAgent,
} from "../../lib/state.js";
import { collectHandoff } from "../../lib/transcript.js";
import { createId, parseOptions, slugify } from "../../lib/utils.js";
import { runAgent, spawnWithInput } from "../../lib/runners.js";
import { renderEvent } from "../../lib/transcript-events.js";
import { createPacket } from "../../lib/packets.js";

async function main() {
  const cwd = process.cwd();
  const tokens = process.argv.slice(2);
  if (tokens.length === 0) return await handleStatus(cwd);

  // Bare `cf @zeus <prompt>` routes like `cf run @zeus <prompt>` (parity with /consensflow:cf @zeus).
  const command = tokens[0].startsWith("@") ? "run" : tokens.shift();
  switch (command) {
    case "status":
    case "state":
      return await handleStatus(cwd);
    case "doctor":
      return await handleDoctor(cwd);
    case "agents":
    case "agent":
      return await handleAgents(tokens, cwd);
    case "run":
    case "ask":
      return await handleRun(tokens, cwd);
    case "help":
    default:
      console.log(helpText());
  }
}

async function handleStatus(cwd) {
  const agents = await loadAgents(cwd);
  const current = await loadCurrent(cwd);
  const session = await loadSession(cwd);
  console.log(
    [
      "# ConsensFlow status",
      "",
      `ConsensFlow home: ${configHome()}`,
      `Agents file: ${agentsPath(cwd)}`,
      `Artifact root for this workspace: ${cfRoot(cwd)}`,
      `Session stash: ${session.transcriptPath ? `transcript tracked (${session.transcriptPath})` : "no transcript tracked yet — handoffs will be empty until the plugin hooks run"}`,
      `Agents: ${agents.length}${driftNote(agents)}`,
      `Latest run: ${current.latestRunId ?? "none"}`,
      "",
      formatAgents(agents, cwd),
    ].join("\n"),
  );
}

async function handleDoctor(cwd) {
  const KIND_BINARY = { pi: "pi", "claude-code": "claude", codex: "codex", opencode: "opencode" };
  const binaries = ["pi", "claude", "codex", "opencode"];
  const agents = await loadAgents(cwd).catch(() => []);
  const neededBy = {};
  for (const p of agents) {
    const binary = KIND_BINARY[p.kind];
    if (binary) (neededBy[binary] ??= []).push(`@${p.id}`);
  }
  const rows = [];
  for (const binary of binaries) {
    const result = await spawnWithInput(binary, ["--version"], { cwd, timeoutMs: 5000 });
    rows.push({ binary, ok: result.exitCode === 0, output: (result.stdout || result.stderr || "").trim(), neededBy: neededBy[binary] ?? [] });
  }
  const imageAgents = agents.filter((p) => p.kind === "image").map((p) => `@${p.id}`);
  const missing = rows.filter((row) => !row.ok && row.neededBy.length > 0);
  const lines = [
    "# ConsensFlow doctor",
    "",
    `ConsensFlow home: ${configHome()}`,
    `Agents file: ${agentsPath(cwd)}`,
    "",
    ...rows.map((row) => {
      const need = row.neededBy.length > 0 ? ` — needed by ${row.neededBy.join(", ")}` : " — not used by any agent";
      return `- ${row.ok ? "✓" : "✗"} ${row.binary}: ${row.output || "not available"}${need}`;
    }),
  ];
  if (imageAgents.length > 0) {
    const codexAuth = await loadCodexAuth().catch(() => null);
    lines.push("", `- ${codexAuth ? "✓" : "✗"} codex login (gpt-image-2 backend) — needed by ${imageAgents.join(", ")}${codexAuth ? "" : ` — run \`codex login\` (checked ${codexAuthPath()})`}`);
  }
  if (missing.length > 0) {
    lines.push("", "Missing engines that configured agents need:", ...missing.map((row) => `  - ${row.binary} (needed by ${row.neededBy.join(", ")})`));
  }
  console.log(lines.join("\n"));
}

async function handleAgents(tokens, cwd) {
  await ensureCfDirs(cwd);
  const sub = tokens.shift() ?? "list";
  if (sub === "list") {
    console.log(formatAgents(await loadAgents(cwd), cwd));
    return;
  }
  if (sub === "presets" || sub === "preset") {
    console.log(formatPresets());
    return;
  }
  if (sub === "sync") {
    const parsed = parseOptions(tokens);
    const result = await syncAgentsWithPresets(cwd, { dryRun: Boolean(parsed.flags["dry-run"]) });
    console.log(formatSync(result));
    return;
  }

  if (sub === "show") {
    const ref = tokens[0];
    if (!ref) throw new Error("Usage: /consensflow:agents show @name");
    const agent = await getAgent(cwd, ref);
    if (!agent) throw new Error(`Unknown agent: ${ref}`);
    console.log(`# ${agent.name}\n\n\`\`\`json\n${JSON.stringify(agent, null, 2)}\n\`\`\``);
    return;
  }
  if (sub === "remove" || sub === "rm") {
    const ref = tokens[0];
    if (!ref) throw new Error("Usage: /consensflow:agents remove @name");
    const removed = await removeAgent(cwd, ref);
    console.log(removed ? `Removed ${ref}.` : `No agent matched ${ref}.`);
    return;
  }
  if (sub === "add") {
    const parsed = parseOptions(tokens);
    const presetRef = parsed.positional[0];

    // Add every preset at once.
    if (presetRef === "all") {
      // `--name`/`--id` would make every preset derive the same id and overwrite each other.
      // Only allow flags that apply uniformly to a bulk add.
      assertAllowedFlags(parsed.flags, ["cwd", "description"], "preset add all");
      const agents = [];
      for (const presetId of listPresetIds()) {
        agents.push(await upsertAgent(cwd, agentFromPreset(presetId, presetOverrides(parsed.flags))));
      }
      console.log(`Saved ${agents.length} presets in ${agentsPath(cwd)}.\n\n${agents.map(formatAgentLine).join("\n")}`);
      return;
    }

    // Preset path: positional names a known preset; --name optionally renames it.
    if (presetRef && getPreset(presetRef)) {
      assertAllowedFlags(parsed.flags, PRESET_OVERRIDE_FLAGS, "preset add");
      const agent = await upsertAgent(cwd, agentFromPreset(presetRef, presetOverrides(parsed.flags)));
      const from = agent.preset && agent.preset !== agent.id ? ` from preset \`${agent.preset}\`` : "";
      console.log(`Saved agent @${agent.id}${from} in ${agentsPath(cwd)}.\n\n${formatAgentLine(agent)}`);
      return;
    }

    // Custom path: explicit custom intent via --name or any backend flag. A positional serves as the name.
    if (stringFlag(parsed.flags.name) !== undefined || hasCustomShape(parsed.flags)) {
      assertAllowedFlags(parsed.flags, CUSTOM_ADD_FLAGS, "custom add");
      const name = stringFlag(parsed.flags.name) ?? presetRef;
      if (!name) throw new Error("Custom agent needs a name: /consensflow:agents add --name <name> --kind <kind> --model <model> ...");
      const agent = await upsertAgent(cwd, customAgentInput(name, parsed.flags));
      console.log(`Saved custom agent @${agent.id} in ${agentsPath(cwd)}.\n\n${formatAgentLine(agent)}`);
      return;
    }

    if (presetRef) {
      throw new Error(
        `Unknown preset: ${presetRef}\n\nPresets: ${listPresetIds().join(", ")} (rename any with --name).\n\nOr create a custom agent:\n  /consensflow:agents add --name <name> --kind <pi|claude-code|codex|opencode|image> --model <model> [--effort <e>]`,
      );
    }
    throw new Error(addUsage());
  }
  throw new Error("Usage: /consensflow:agents list|presets|add|show|remove|sync");
}

async function handleRun(tokens, cwd) {
  // The CC analog of pi agents running with --no-extensions: an agent subprocess must
  // not consult further agents (no fan-out, no recursion).
  if (process.env.CONSENSFLOW_CHILD) {
    throw new Error("Nested ConsensFlow runs are disabled inside agent subprocesses.");
  }
  await ensureCfDirs(cwd);
  const parsed = parseRunOptions(tokens);
  const positional = [...parsed.positional];
  const ref = positional.shift();
  if (!ref || !ref.startsWith("@")) {
    throw new Error("Usage: /consensflow @name <prompt> — or via the Bash tool: run @name <prompt> [--brief <who this run is for>] [--prompt-file <file>] [--context <note>] [--no-handoff] [--json]");
  }
  if (positional[0]?.startsWith("@")) {
    throw new Error("ConsensFlow sends to one agent at a time. Ask one, read its answer, then ask another.");
  }

  const agent = await getAgent(cwd, ref);
  if (!agent) {
    const known = (await loadAgents(cwd)).map((p) => `@${p.id}`).join(", ") || "none configured — add one with `/consensflow:agents add <preset>` (see `/consensflow:presets`)";
    throw new Error(`Unknown agent: @${slugify(String(ref).replace(/^@+/, ""))}. Configured: ${known}`);
  }

  let prompt = positional.join(" ");
  if (stringFlag(parsed.flags.prompt) !== undefined) prompt = String(parsed.flags.prompt);
  if (stringFlag(parsed.flags["prompt-file"]) !== undefined) prompt = await fs.readFile(String(parsed.flags["prompt-file"]), "utf8");
  prompt = prompt.trim();
  if (!prompt) throw new Error(`Prompt is required after @${agent.id} (inline, --prompt, or --prompt-file).`);

  // Image agents bypass the CLI runner: prompt-only (no packet/handoff), Codex backend.
  if (agent.kind === "image") return await runImageAgent(cwd, agent, prompt, parsed.flags);

  // Only an unexpectedly-empty handoff is surfaced in the run output — a silently-missing session
  // stash would otherwise look identical to a full handoff from the agent's answer alone.
  let handoff = "";
  let handoffSummary = "skipped (--no-handoff)";
  if (flagBool(parsed.flags, "handoff") ?? true) {
    handoff = stringFlag(parsed.flags["handoff-file"]) !== undefined
      ? await fs.readFile(String(parsed.flags["handoff-file"]), "utf8")
      : await collectHandoff(cwd);
    handoffSummary = handoff.trim()
      ? `attached (${Math.max(1, Math.round(Buffer.byteLength(handoff, "utf8") / 1024))} KB)`
      : "empty — no session transcript stashed for this workspace (are the plugin hooks running?)";
  }

  const packet = await createPacket({
    cwd,
    agent,
    kind: "ask",
    task: prompt,
    brief: stringFlag(parsed.flags.brief),
    extraContext: stringFlag(parsed.flags.context),
    handoff,
  });
  // PRIMARY observability path: streaming is ALWAYS on — the thinking must stay visible, never run a
  // agent without it (--stream/--no-stream are accepted but no longer gate this). Render
  // normalized events to stdout as they arrive so the lead relays the thinking / tool calls / answer
  // live. Suppressed ONLY under --json, where streamed lines would corrupt the machine output.
  let inDelta = false;
  let sawDelta = false;
  const onEvent = parsed.flags.json !== true
    ? (event) => {
      if (event.kind === "delta") { process.stdout.write(event.text); inDelta = true; sawDelta = true; return; } // pi reasoning/text, flowing like its own UI
      // Once pi has streamed deltas, its message_end thinking/text blocks are redundant with what
      // already flowed — skip them live (tool calls still render; the trail keeps them for timeouts).
      if (sawDelta && (event.kind === "thinking" || event.kind === "text")) return;
      const line = renderEvent(event);
      if (line) { process.stdout.write(`${inDelta ? "\n" : ""}${line}\n`); inDelta = false; }
    }
    : undefined;
  const result = await runAgent({ cwd, agent, packet, kind: "ask", onEvent });
  result.handoffSummary = handoffSummary;

  if (inDelta) process.stdout.write("\n"); // a trailing pi reasoning delta shouldn't butt against the final answer header
  if (parsed.flags.json === true) {
    console.log(JSON.stringify(result, null, 2));
    return;
  }
  // Always print the parsed final result after the child exits, even while streaming. Some engine
  // streams omit answer text until the terminal summary. This mirrors Pi: live crumbs are
  // best-effort; the final reply is durable.
  console.log(renderRunResult(result));
}

export function parseRunOptions(tokens) {
  const positional = [];
  const flags = {};
  const valueFlags = new Set(["brief", "context", "prompt", "prompt-file", "handoff-file", "image"]);
  const booleanFlags = new Set(["stream", "no-stream", "json", "rw", "handoff", "no-handoff"]);
  // Repeatable flags collect into an array: `--image a.png --image b.png` → ["a.png", "b.png"].
  const multiValueFlags = new Set(["image"]);
  const setValue = (name, value) => {
    if (multiValueFlags.has(name)) (flags[name] ??= []).push(value);
    else flags[name] = value;
  };
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i];
    if (!token.startsWith("--") || token === "--") {
      positional.push(token);
      continue;
    }
    const raw = token.slice(2);
    const eq = raw.indexOf("=");
    const name = eq >= 0 ? raw.slice(0, eq) : raw;
    if (eq >= 0) {
      if (valueFlags.has(name)) setValue(name, raw.slice(eq + 1));
      else if (booleanFlags.has(name)) flags[name] = raw.slice(eq + 1) !== "false";
      else positional.push(token);
      continue;
    }
    if (booleanFlags.has(name)) {
      flags[name] = true;
      continue;
    }
    if (valueFlags.has(name)) {
      const next = tokens[i + 1];
      if (next === undefined || next.startsWith("--")) throw new Error(`--${name} requires a value`);
      setValue(name, next);
      i += 1;
      continue;
    }
    // Unknown --flags are treated as prompt text, so pasted commands like `git diff --stat` are
    // not stripped from the agent task.
    positional.push(token);
  }
  return { positional, flags };
}

// Image generation doesn't fit the text-CLI runner: it calls the Codex Responses backend
// (gpt-image-2) over HTTP, riding the Codex CLI's ChatGPT login. The image model gets the
// prompt verbatim (no packet/handoff) — an image model can't use the transcript.
async function runImageAgent(cwd, agent, prompt, flags) {
  const imagePaths = Array.isArray(flags.image) ? flags.image : flags.image ? [flags.image] : [];
  const result = await runImageRun({ cwd, agent, prompt, imagePaths });
  console.log(flags.json === true ? JSON.stringify(result, null, 2) : renderImageRun(result));
}
// Tri-state flag pair: --<name> → true, --no-<name> → false, neither → undefined.
function flagBool(flags, name) {
  if (flags[`no-${name}`] === true) return false;
  if (flags[name] === true) return true;
  return undefined;
}

const PRESET_OVERRIDE_FLAGS = ["name", "id", "cwd", "description"];
const CUSTOM_ADD_FLAGS = ["name", "id", "kind", "model", "provider", "effort", "thinking", "skills", "skillsPolicy", "harness", "cwd", "maxTurns", "description"];
const CUSTOM_SHAPE_FLAGS = ["kind", "model", "provider", "effort", "thinking", "skills", "skillsPolicy", "harness", "maxTurns"];

function assertAllowedFlags(flags, allowed, context) {
  const allowedSet = new Set(allowed);
  const rejected = Object.keys(flags).filter((flag) => !allowedSet.has(flag));
  if (rejected.length > 0) {
    throw new Error(`Unsupported ${context} option(s): ${rejected.map((flag) => `--${flag}`).join(", ")}. Allowed: ${allowed.map((flag) => `--${flag}`).join(", ")}.`);
  }
}

function hasCustomShape(flags) {
  return CUSTOM_SHAPE_FLAGS.some((flag) => stringFlag(flags[flag]) !== undefined);
}

function stringFlag(value) {
  if (value === undefined || value === null || value === true) return undefined;
  const trimmed = String(value).trim();
  return trimmed || undefined;
}

function presetOverrides(flags) {
  return { name: flags.name, id: flags.id, cwd: flags.cwd, description: flags.description };
}

function customAgentInput(name, flags) {
  return {
    name,
    id: flags.id,
    kind: flags.kind,
    model: flags.model,
    provider: flags.provider,
    effort: flags.effort,
    thinking: flags.thinking,
    skillsPolicy: flags.skills ?? flags.skillsPolicy,
    harness: flags.harness,
    cwd: flags.cwd,
    maxTurns: flags.maxTurns,
    description: flags.description,
  };
}

function addUsage() {
  return [
    "Usage:",
    "  /consensflow:agents add <preset> [--name <name>]   # from a preset, optionally renamed",
    "  /consensflow:agents add all                         # every preset",
    "  /consensflow:agents add --name <name> --kind <pi|claude-code|codex|opencode|image> --model <model> [--effort <e>] [--thinking <t>] [--cwd <subdir>]",
    "",
    `Presets: ${listPresetIds().join(", ")}`,
  ].join("\n");
}

function formatAgents(agents, cwd = process.cwd()) {
  if (agents.length === 0) {
    return [
      "# ConsensFlow agents",
      "",
      `Agents file: ${agentsPath(cwd)}`,
      "",
      "No agents configured yet.",
      "",
      "Create agents:",
      "```text",
      "/consensflow:presets                                    # list the curated presets",
      "/consensflow:agents add zeus                      # add a preset",
      "/consensflow:agents add zeus --name Deepreview    # preset backend, custom name",
      "/consensflow:agents add all                       # every preset",
      "/consensflow:agents add --name Builder --kind codex --model gpt-5.6-sol",
      "```",
    ].join("\n");
  }
  const drift = driftNote(agents);
  return [
    "# ConsensFlow agents",
    "",
    `Agents file: ${agentsPath(cwd)}`,
    ...(drift ? [`Catalog:${drift}`] : []),
    "",
    ...agents.map(formatAgentLine),
  ].join("\n");
}

// One-line "you are behind the catalog" hint, appended wherever the roster is summarised.
function driftNote(agents) {
  const drifted = driftedAgents(agents);
  if (drifted.length === 0) return "";
  return `  (${drifted.length} behind the catalog — run \`/consensflow:agents sync\`)`;
}

function formatSync(result) {
  const lines = ["# ConsensFlow agents sync", ""];
  if (result.synced.length === 0) {
    lines.push(`All ${result.total} agents already match the catalog.`);
  } else {
    for (const entry of result.synced) {
      for (const change of entry.changes) {
        // Descriptions are long; report them as a fact rather than a diff.
        lines.push(change.field === "description" ? `@${entry.id}  description updated` : `@${entry.id}  ${change.field}  ${change.from ?? "(none)"} → ${change.to ?? "(none)"}`);
      }
    }
    lines.push("");
    lines.push(`${result.dryRun ? "Would sync" : "Synced"} ${result.synced.length} agent${result.synced.length === 1 ? "" : "s"} (${result.total - result.synced.length} already current).`);
  }
  if (result.orphans.length > 0) lines.push("", `Left pinned (preset no longer in the catalog): ${result.orphans.join(", ")}`);
  return lines.join("\n");
}

function formatAgentLine(p) {
  const model = p.model ? ` model=${p.model}` : "";
  const effort = p.effort ? ` effort=${p.effort}` : p.thinking ? ` thinking=${p.thinking}` : "";
  const cwd = p.cwd ? ` cwd=${p.cwd}` : "";
  const skills = p.kind === "pi" ? ` skills=${p.skillsPolicy ?? "default"}` : "";
  const preset = p.preset ? ` preset=${p.preset}` : "";
  const head = `- @${p.id} (${p.kind}${model}${effort}${cwd}${skills}${preset})`;
  return p.description ? `${head}\n    ${p.description}` : head;
}

// Just the answer on a clean run. Diagnostics appear only when they matter: the run failed or the
// handoff was unexpectedly empty. Every agent now runs read-write, so the inspect-your-repo
// nudge always shows. Full metadata stays in result.json (and `--json`).
function renderRunResult(result) {
  const lines = [`# @${result.agent.id}`];
  if (result.exitCode !== 0) {
    lines.push("", `Run failed: exit ${result.exitCode} — artifacts: ${result.runDir}`);
  }
  if (result.handoffSummary?.startsWith("empty")) lines.push("", `Handoff: ${result.handoffSummary}`);
  lines.push("", "> Full-permission run: this agent ran unsandboxed — it could edit any file, run any command, and reach the network. Inspect what changed (e.g. `git status` / `git diff`) before keeping or building on it.");
  lines.push("", result.output);
  return lines.join("\n");
}

function helpText() {
  return `# ConsensFlow help

Ask one named agent at a time. Each agent gets the current session as a handoff
plus your prompt, and answers conversationally.

Ask an agent:

\`\`\`text
@zeus What do you think about this approach?       # mention it in your prompt — the plugin routes it
/consensflow:cf @zeus What do you think?           # explicit slash command
\`\`\`

Manage agents (shared across Claude Code and Pi, ${agentsPath(process.cwd())}):

\`\`\`text
/consensflow:presets                                    # list the curated presets
/consensflow:agents                               # list configured agents
/consensflow:agents add zeus                      # add a preset
/consensflow:agents add zeus --name Deepreview    # preset backend, your own name
/consensflow:agents add all                       # every preset
/consensflow:agents add --name Builder --kind codex --model gpt-5.6-sol --effort high \\
                                # fully custom, write-capable
/consensflow:agents show @zeus
/consensflow:agents remove @zeus
/consensflow:status                                     # roster + latest run
/consensflow:doctor                                     # engine CLI health check
\`\`\`

For the lead (via the Bash tool), the CLI subcommands are \`status\` | \`doctor\` |
\`agents list|presets|add|show|remove|sync\` | \`run @name <prompt>\`, with run flags \`--prompt <text>\` |
\`--prompt-file <file>\` | \`--context <note>\` | \`--no-handoff\` | \`--image <path>\` (image agents) | \`--json\`.

Rules:

- Send to one agent at a time.
- One-shot: agents do not remember previous calls; each call re-sends the current session handoff.
- The current Claude Code session remains the lead and decides what to implement.
`;
}

// Run only when invoked as the CLI entry point, so tests can import the pure helpers above.
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    await main();
  } catch (error) {
    console.error(`ConsensFlow error: ${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 1;
  }
}
