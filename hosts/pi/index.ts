import fs from "node:fs/promises";
import path from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-harness";
import { Type } from "typebox";
import { decodeChatGptAccountId, generateImage, imageFileToDataUrl, IMAGE_TRIGGER_DEFAULT, saveImagePng } from "../lib/image.js";
import { driftedAgents, formatPresets, getPreset, listPresetIds, agentFromPreset } from "../lib/presets.js";
import {
  cfRoot,
  configHome,
  configRoot,
  ensureCfDirs,
  getAgent,
  loadCurrent,
  loadAgents,
  agentsPath,
  recordLatestRun,
  removeAgent,
  runsRoot,
  saveSession,
  syncAgentsWithPresets,
  upsertAgent,
} from "../lib/state.js";
import { createId, parseOptions, parseAgentPrompt, slugify, tokenize } from "../lib/utils.js";
import { runNamedAgent } from "../lib/workflows.js";
import { renderEvent } from "../lib/transcript-events.js";

const EXT = "consensflow";

export default async function consensflow(pi: ExtensionAPI) {
  pi.on("session_start", async (_event, ctx) => {
    const agents = await loadAgents(ctx.cwd).catch(() => []);
    ctx.ui.setStatus(EXT, `CF ${agents.length} agent${agents.length === 1 ? "" : "s"}`);
  });

  // Nothing watches input. The extension used to stash the conversation here
  // so a CLI spawn could attach it; the handoff is the lead's to pass now,
  // with --handoff-file, which is the same in every harness.

  registerCoreCommands(pi);

  // No ConsensFlow tools here.
  //
  // pi's lead used to have `cf_list_agents` and `cf_run_agent`, which Claude
  // Code and a cmux pane never had — so the same request took a different
  // shape depending on where it was made, and the tool's own description was
  // a third place to keep the rules current. The lead reads the skill and runs
  // `cf run` now, in every harness. `/consensflow:*` remains for driving a run
  // by hand.

}

type CoreCommandSpec = {
  name: string;
  description: string;
  toCfArgs: (args: string) => string;
};

const CORE_COMMANDS: CoreCommandSpec[] = [
  // Match Claude Code's discoverable command surface: only /consensflow:* commands.
  {
    name: "consensflow:cf",
    description: "ConsensFlow: manage named agents or send one prompt to one agent",
    toCfArgs: (args) => args,
  },
  {
    name: "consensflow:status",
    description: "ConsensFlow: show configured agents and the latest run",
    toCfArgs: () => "status",
  },
  {
    name: "consensflow:doctor",
    description: "ConsensFlow: check which engine CLIs are installed and working",
    toCfArgs: () => "doctor",
  },
  {
    name: "consensflow:presets",
    description: "ConsensFlow: list the curated agent presets",
    toCfArgs: () => "agents presets",
  },
  {
    name: "consensflow:agents",
    description: "ConsensFlow: list or manage your agents (add, show, remove, presets)",
    toCfArgs: (args) => prefixedArgs("agents", args),
  },
];

function registerCoreCommands(pi: ExtensionAPI) {
  for (const command of CORE_COMMANDS) {
    pi.registerCommand(command.name, {
      description: command.description,
      handler: async (args, ctx) => handleCf(command.toCfArgs(String(args ?? "")), ctx, pi),
    });
  }
}

function prefixedArgs(prefix: string, args: string) {
  const trimmed = String(args ?? "").trim();
  return trimmed ? `${prefix} ${trimmed}` : prefix;
}

async function handleCf(args: string, ctx: any, pi: ExtensionAPI) {
  try {
    await ensureCfDirs(ctx.cwd);
    const tokens = tokenize(args);
    if (tokens.length === 0) return await handleStatus(ctx, pi);

    const known = await knownAgentKeys(ctx.cwd);
    const commandName = tokens[0]?.toLowerCase();
    if (!CF_SUBCOMMANDS.has(commandName)) {
      const directPrompt = parseRunPrompt(tokens, known);
      if (directPrompt) return await handleAgentPrompt(directPrompt, ctx, pi, ctx.signal);
    }

    const command = tokens.shift() ?? "status";
    switch (command) {
      case "status":
      case "state":
        return await handleStatus(ctx, pi);
      case "doctor":
        return await handleDoctor(ctx, pi);
      case "agents":
      case "agent":
        return await handleAgents(tokens, ctx, pi);
      case "run":
      case "ask":
      case "to": {
        const parsed = parseRunPrompt(tokens, known);
        if (!parsed) throw new Error("Usage: /consensflow:cf @name <prompt> or /consensflow:cf ask @name <prompt>");
        return await handleAgentPrompt(parsed, ctx, pi, ctx.signal);
      }
      case "help":
      default:
        return sendCfMessage(pi, helpText(), { command: "help" });
    }
  } catch (error) {
    reportCfError(pi, ctx, error);
  }
}

async function handleStatus(ctx: any, pi: ExtensionAPI) {
  const agents = await loadAgents(ctx.cwd);
  const current = await loadCurrent(ctx.cwd);
  const markdown = [
    "# ConsensFlow status",
    "",
    `ConsensFlow home: ${configHome()}`,
    `Agents file: ${agentsPath(ctx.cwd)}`,
    `Artifact root for this workspace: ${cfRoot(ctx.cwd)}`,
    `Agents: ${agents.length}${driftNote(agents)}`,
    `Latest run: ${current.latestRunId ?? "none"}`,
    "",
    formatAgents(agents, ctx.cwd),
  ].join("\n");
  sendCfMessage(pi, markdown, { agents, current, ...storeDetails(ctx.cwd), artifactRoot: cfRoot(ctx.cwd) });
}

async function handleDoctor(ctx: any, pi: ExtensionAPI) {
  const KIND_BINARY: Record<string, string> = { pi: "pi", "claude-code": "claude", codex: "codex", opencode: "opencode" };
  const binaries = ["pi", "claude", "codex", "opencode"];
  const agents = (await loadAgents(ctx.cwd).catch(() => [])) as any[];
  const neededBy: Record<string, string[]> = {};
  for (const p of agents) {
    const binary = KIND_BINARY[p.kind];
    if (binary) (neededBy[binary] ??= []).push(`@${p.id}`);
  }
  const rows = [];
  for (const binary of binaries) {
    const result = await pi.exec(binary, ["--version"], { timeout: 5000 });
    rows.push({ binary, ok: result.code === 0, output: (result.stdout || result.stderr || "").trim(), neededBy: neededBy[binary] ?? [] });
  }
  const imageAgents = agents.filter((p) => p.kind === "image").map((p) => `@${p.id}`);
  const missing = rows.filter((row) => !row.ok && row.neededBy.length > 0);
  const lines = [
    "# ConsensFlow doctor",
    "",
    `ConsensFlow home: ${configHome()}`,
    `Agents file: ${agentsPath(ctx.cwd)}`,
    "",
    ...rows.map((row) => {
      const need = row.neededBy.length > 0 ? ` — needed by ${row.neededBy.join(", ")}` : " — not used by any agent";
      return `- ${row.ok ? "✓" : "✗"} ${row.binary}: ${row.output || "not available"}${need}`;
    }),
  ];
  if (imageAgents.length > 0) {
    lines.push("", `- image agents (${imageAgents.join(", ")}) need an \`openai-codex\` login (\`/login\` → ChatGPT Plus/Pro), not a CLI binary.`);
  }
  if (missing.length > 0) {
    lines.push("", "Missing engines that configured agents need:", ...missing.map((row) => `  - ${row.binary} (needed by ${row.neededBy.join(", ")})`));
  }
  sendCfMessage(pi, lines.join("\n"), { rows, imageAgents, ...storeDetails(ctx.cwd) });
}

async function handleAgents(tokens: string[], ctx: any, pi: ExtensionAPI) {
  const sub = tokens.shift() ?? "list";
  if (sub === "list") {
    const agents = await loadAgents(ctx.cwd);
    return sendCfMessage(pi, formatAgents(agents, ctx.cwd), { agents, ...storeDetails(ctx.cwd) });
  }
  if (sub === "presets" || sub === "preset") {
    return sendCfMessage(pi, formatPresets(), { presets: listPresetIds() });
  }
  if (sub === "sync") {
    const parsed = parseOptions(tokens);
    const result = await syncAgentsWithPresets(ctx.cwd, { dryRun: Boolean(parsed.flags["dry-run"]) });
    if (result.synced.length > 0 && !result.dryRun) ctx.ui.notify(`Synced ${result.synced.length} agent${result.synced.length === 1 ? "" : "s"} with the catalog`, "info");
    return sendCfMessage(pi, formatSync(result), { ...result, ...storeDetails(ctx.cwd) });
  }

  if (sub === "show") {
    const ref = tokens[0];
    if (!ref) throw new Error("Usage: /consensflow:agents show @name");
    const agent = await getAgent(ctx.cwd, ref);
    if (!agent) throw new Error(`Unknown agent: ${ref}`);
    return sendCfMessage(pi, `# ${agent.name}\n\n\`\`\`json\n${JSON.stringify(agent, null, 2)}\n\`\`\``, { agent });
  }
  if (sub === "remove" || sub === "rm") {
    const ref = tokens[0];
    if (!ref) throw new Error("Usage: /consensflow:agents remove @name");
    const removed = await removeAgent(ctx.cwd, ref);
    ctx.ui.notify(removed ? `Removed ${ref}` : `No agent matched ${ref}`, removed ? "info" : "warning");
    return sendCfMessage(pi, removed ? `Removed ${ref}.` : `No agent matched ${ref}.`, { removed, ref });
  }
  if (sub === "add") {
    const parsed = parseOptions(tokens);
    const presetRef = parsed.positional[0];

    // Add every preset at once.
    if (presetRef === "all") {
      // `--name`/`--id` would make every preset derive the same id and overwrite each other (saving
      // one agent while reporting "Saved 24"). Only allow flags that apply uniformly to a bulk add.
      assertAllowedFlags(parsed.flags, ["cwd", "description"], "preset add all");
      const agents = [];
      for (const presetId of listPresetIds()) {
        agents.push(await upsertAgent(ctx.cwd, agentFromPreset(presetId, presetOverrides(parsed.flags))));
      }
      ctx.ui.notify(`Saved ${agents.length} ConsensFlow agents`, "info");
      return sendCfMessage(pi, `Saved presets in ${agentsPath(ctx.cwd)}.\n\n${agents.map(formatAgentLine).join("\n")}`, { agents, ...storeDetails(ctx.cwd) });
    }

    // Preset path: positional names a known preset; --name optionally renames it.
    if (presetRef && getPreset(presetRef)) {
      assertPresetOverrideFlags(parsed.flags);
      const agent = await upsertAgent(ctx.cwd, agentFromPreset(presetRef, presetOverrides(parsed.flags)));
      ctx.ui.notify(`Saved @${agent.id}`, "info");
      const from = agent.preset && agent.preset !== agent.id ? ` from preset \`${agent.preset}\`` : "";
      return sendCfMessage(pi, `Saved agent @${agent.id}${from} in ${agentsPath(ctx.cwd)}.\n\n${formatAgentLine(agent)}`, { agent, ...storeDetails(ctx.cwd) });
    }

    // Custom path: explicit custom intent via --name or any backend flag. A positional serves as the name.
    if (stringFlag(parsed.flags.name) !== undefined || hasCustomShape(parsed.flags)) {
      assertCustomAddFlags(parsed.flags);
      const name = stringFlag(parsed.flags.name) ?? presetRef;
      if (!name) throw new Error("Custom agent needs a name: /consensflow:agents add --name <name> --kind <kind> --model <model> ...");
      const agent = await upsertAgent(ctx.cwd, customAgentInput(name, parsed.flags));
      ctx.ui.notify(`Saved @${agent.id}`, "info");
      return sendCfMessage(pi, `Saved custom agent @${agent.id} in ${agentsPath(ctx.cwd)}.\n\n${formatAgentLine(agent)}`, { agent, ...storeDetails(ctx.cwd) });
    }

    if (presetRef) {
      throw new Error(`Unknown preset: ${presetRef}\n\nPresets: ${listPresetIds().join(", ")} (rename any with --name).\n\nOr create a custom agent:\n  /consensflow:agents add --name <name> --kind <pi|claude-code|codex|opencode|image> --model <model> [--effort <e>]`);
    }
    throw new Error(addUsage());
  }
  throw new Error("Usage: /consensflow:agents list|presets|add|show|remove|sync");
}

async function handleAgentPrompt(parsed: AgentPrompt, ctx: any, pi: ExtensionAPI, signal?: AbortSignal) {
  if (parsed.error) throw new Error(parsed.error);
  const agent = await getAgent(ctx.cwd, parsed.agent);
  if (!agent) throw new Error(`Unknown agent: @${parsed.agent}`);
  if (agent.kind === "image") return await runImageAgent(agent, parsed.prompt, ctx, pi, signal, parsed.images);
  ctx.ui.notify(`Asking @${agent.id}...`, "info");
  // No conversation unless one is handed over. pi could read its own session
  // in process and used to, which made `/consensflow:cf` behave differently
  // from `cf run` in the same window — and pi differently from every other
  // harness. The lead passes what it wants shared, everywhere.
  const includeHandoff = false;
  const handoff = "";
  const result = await runNamedAgent({
    cwd: ctx.cwd,
    agentRef: agent,
    kind: "ask",
    task: parsed.prompt,
    extraContext: parsed.context,
    handoff,
    signal,
    // Direct @mention and /consensflow:cf runs should be visible in the main session too, not
    // just as a final answer after a long child process. Stream the normalized trail as
    // lightweight custom messages; handoff serialization ignores these stream crumbs and keeps
    // only the final agent reply for cross-pollination.
    onEvent: (event: any) => sendCfStreamEvent(pi, agent, event),
  });
  result.handoffSummary = summarizeHandoff(handoff, includeHandoff);
  // Record the prompt in details so later agents' handoffs can reconstruct this exchange
  // (the @mention input was "handled" and is never stored as a normal session message).
  sendCfMessage(pi, renderRunResult(result), { ...result, prompt: parsed.prompt });
}

// --- Image agents (kind: "image") ---------------------------------
// Image generation doesn't fit the text-CLI runner: it calls the Codex Responses
// backend (gpt-image-2) over HTTP — reusing the openai-codex login — and returns
// an image. Handled here, not in runners.js, because it needs ctx.modelRegistry.
// The image model gets the prompt verbatim (no packet/handoff).
async function generateImageArtifact(ctx: any, agent: any, prompt: string, signal?: AbortSignal, imagePaths: string[] = []) {
  const token = await ctx?.modelRegistry?.getApiKeyForProvider?.("openai-codex");
  if (!token) {
    throw new Error("No openai-codex login found. Run /login and pick ChatGPT Plus/Pro (Codex) to use image agents.");
  }
  const accountId = decodeChatGptAccountId(token);
  await ensureCfDirs(ctx.cwd);
  const runId = createId("image");
  const runDir = path.join(runsRoot(ctx.cwd), runId);
  await fs.mkdir(runDir, { recursive: true });
  const triggerModel = agent.model || IMAGE_TRIGGER_DEFAULT;
  // Optional reference images become input_image parts so gpt-image-2 can edit/condition on them.
  const images = await Promise.all((imagePaths ?? []).map((p) => imageFileToDataUrl(p)));
  const image = await generateImage({ token, accountId, prompt, triggerModel, images, signal });
  const savedPath = await saveImagePng(image.base64, runDir, "image.png");
  await fs.writeFile(
    path.join(runDir, "result.json"),
    `${JSON.stringify({ runId, savedPath, triggerModel, backend: "gpt-image-2", referenceImages: imagePaths ?? [], revisedPrompt: image.revisedPrompt, responseId: image.responseId, agent: { id: agent.id, kind: agent.kind } }, null, 2)}\n`,
    "utf8",
  );
  await recordLatestRun(ctx.cwd, { runId, runDir, agent, kind: "image" });
  return { runId, runDir, savedPath, mimeType: "image/png", base64: image.base64, revisedPrompt: image.revisedPrompt, referenceImages: imagePaths ?? [] };
}

function imageSummary(agent: any, r: { savedPath: string; revisedPrompt?: string; referenceImages?: string[] }) {
  return [
    `# @${agent.id}`,
    "",
    "Generated an image with **gpt-image-2** (via your openai-codex login).",
    r.referenceImages?.length ? `Reference image(s): ${r.referenceImages.join(", ")}` : undefined,
    r.revisedPrompt ? `Revised prompt: ${r.revisedPrompt}` : undefined,
    `Saved: ${r.savedPath}`,
  ]
    .filter(Boolean)
    .join("\n");
}

async function runImageAgent(agent: any, prompt: string, ctx: any, pi: ExtensionAPI, signal?: AbortSignal, imagePaths: string[] = []) {
  ctx.ui.notify(`Generating image with @${agent.id} (gpt-image-2)...`, "info");
  const r = await generateImageArtifact(ctx, agent, prompt, signal, imagePaths);
  pi.sendMessage({
    customType: EXT,
    content: [
      { type: "text", text: imageSummary(agent, r) },
      { type: "image", data: r.base64, mimeType: r.mimeType },
    ],
    display: true,
    details: { runId: r.runId, runDir: r.runDir, savedPath: r.savedPath, revisedPrompt: r.revisedPrompt, agent, prompt, kind: "image" },
  });
}

type AgentPrompt =
  | { agent: string; prompt: string; error?: undefined; context?: string; includeHandoff?: boolean; images?: string[] }
  | { agent?: undefined; prompt?: undefined; error: string };

const CF_SUBCOMMANDS = new Set(["status", "state", "doctor", "agents", "agent", "run", "ask", "to", "help"]);

function parseRunPrompt(tokens: string[], known: Set<string>): AgentPrompt | null {
  const parsed = parseRunOptions(tokens);
  const prompt = parseAgentPrompt(parsed.positional, known) as AgentPrompt | null;
  if (!prompt || prompt.error) return prompt;
  const images = Array.isArray(parsed.flags.image) ? (parsed.flags.image as string[]) : undefined;
  return {
    ...prompt,
    context: stringFlag(parsed.flags.context),
    includeHandoff: flagBool(parsed.flags, "handoff") ?? true,
    images,
  };
}

function parseRunOptions(tokens: string[]) {
  const positional: string[] = [];
  const flags: Record<string, unknown> = {};
  const valueFlags = new Set(["context", "image"]);
  const booleanFlags = new Set(["rw", "handoff", "no-handoff"]);
  // Repeatable flags collect into an array: `--image a.png --image b.png` → ["a.png", "b.png"].
  const multiValueFlags = new Set(["image"]);
  const setValue = (name: string, value: string) => {
    if (multiValueFlags.has(name)) ((flags[name] ??= []) as string[]).push(value);
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
    positional.push(token);
  }
  return { positional, flags };
}

function flagBool(flags: Record<string, unknown>, name: string) {
  if (flags[`no-${name}`] === true) return false;
  if (flags[name] === true) return true;
  return undefined;
}

// Slugified ids + names of every configured agent, matching getAgent's resolution.
async function knownAgentKeys(cwd: string): Promise<Set<string>> {
  const agents = await loadAgents(cwd).catch(() => []);
  return new Set((agents as any[]).flatMap((p) => [p.id, slugify(p.name)]).filter(Boolean));
}

const PRESET_OVERRIDE_FLAGS = ["name", "id", "cwd", "description"];
const CUSTOM_ADD_FLAGS = ["name", "id", "kind", "model", "provider", "effort", "thinking", "skills", "skillsPolicy", "harness", "cwd", "maxTurns", "description"];
const CUSTOM_SHAPE_FLAGS = ["kind", "model", "provider", "effort", "thinking", "skills", "skillsPolicy", "harness", "maxTurns"];

function assertAllowedFlags(flags: Record<string, unknown>, allowed: string[], context: string) {
  const allowedSet = new Set(allowed);
  const rejected = Object.keys(flags).filter((flag) => !allowedSet.has(flag));
  if (rejected.length > 0) {
    throw new Error(`Unsupported ${context} option(s): ${rejected.map((flag) => `--${flag}`).join(", ")}. Allowed: ${allowed.map((flag) => `--${flag}`).join(", ")}.`);
  }
}

function assertPresetOverrideFlags(flags: Record<string, unknown>) {
  assertAllowedFlags(flags, PRESET_OVERRIDE_FLAGS, "preset add");
}

function assertCustomAddFlags(flags: Record<string, unknown>) {
  assertAllowedFlags(flags, CUSTOM_ADD_FLAGS, "custom add");
}

function hasCustomShape(flags: Record<string, unknown>) {
  return CUSTOM_SHAPE_FLAGS.some((flag) => stringFlag(flags[flag]) !== undefined);
}

function stringFlag(value: unknown) {
  if (value === undefined || value === null || value === true) return undefined;
  const trimmed = String(value).trim();
  return trimmed || undefined;
}

function presetOverrides(flags: Record<string, unknown>) {
  return { name: flags.name, id: flags.id, cwd: flags.cwd, description: flags.description };
}

function customAgentInput(name: string, flags: Record<string, unknown>) {
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
    "  /consensflow:agents add <preset> [--name <name>]        # from a preset, optionally renamed",
    "  /consensflow:agents add all                              # every preset",
    "  /consensflow:agents add --name <name> --kind <pi|claude-code|codex|opencode|image> --model <model> [--effort <e>] [--thinking <t>] [--cwd <subdir>]",
    "",
    `Presets: ${listPresetIds().join(", ")}`,
  ].join("\n");
}

function storeDetails(cwd: string) {
  return { configHome: configHome(), agentsPath: agentsPath(cwd), toolArtifactRoot: configRoot() };
}

function formatAgents(agents: any[], cwd = process.cwd()) {
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
function driftNote(agents: any[]) {
  const drifted = driftedAgents(agents);
  if (drifted.length === 0) return "";
  return `  (${drifted.length} behind the catalog — run \`/consensflow:agents sync\`)`;
}

function formatSync(result: any) {
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

function formatAgentLine(p: any) {
  const model = p.model ? ` model=${p.model}` : "";
  const effort = p.effort ? ` effort=${p.effort}` : p.thinking ? ` thinking=${p.thinking}` : "";
  const cwd = p.cwd ? ` cwd=${p.cwd}` : "";
  const skills = p.kind === "pi" ? ` skills=${p.skillsPolicy ?? "default"}` : "";
  const preset = p.preset ? ` preset=${p.preset}` : "";
  const head = `- @${p.id} (${p.kind}${model}${effort}${cwd}${skills}${preset})`;
  return p.description ? `${head}\n    ${p.description}` : head;
}

// The run output reports what context rode along: a silently-empty handoff looks identical to a
// full one from the agent's answer alone.
function summarizeHandoff(handoff: string, included: boolean) {
  if (!included) return "skipped (includeHandoff=false)";
  if (!String(handoff ?? "").trim()) return "empty — no session history to hand off";
  return `attached (${Math.max(1, Math.round(Buffer.byteLength(handoff, "utf8") / 1024))} KB)`;
}

// Just the answer on a clean run. Diagnostics appear only when they matter: the run failed or the
// handoff was unexpectedly empty. Every agent now runs read-write, so the inspect-your-repo
// nudge always shows. Full metadata stays in result.json and the message details.
function renderRunResult(result: any) {
  const lines = [`# @${result.agent.id}`];
  if (result.exitCode !== 0) {
    lines.push("", `Run failed: exit ${result.exitCode} — artifacts: ${result.runDir}`);
  }
  if (result.handoffSummary?.startsWith("empty")) lines.push("", `Handoff: ${result.handoffSummary}`);
  lines.push("", "> Full-permission run: this agent ran unsandboxed — it could edit any file, run any command, and reach the network. Inspect what changed (e.g. `git status` / `git diff`) before keeping or building on it.");
  lines.push("", result.output);
  return lines.join("\n");
}

function sendCfMessage(pi: ExtensionAPI, content: string, details?: any) {
  pi.sendMessage({ customType: EXT, content, display: true, details });
}

function sendCfStreamEvent(pi: ExtensionAPI, agent: any, event: any) {
  const line = renderEvent(event);
  if (!line) return;
  sendCfMessage(pi, line, { streamEvent: true, agent: { id: agent.id, kind: agent.kind } });
}

// Single error surface for every entry path (the /consensflow router and the @mention input
// handler) so a typo or a runner/login failure always gets the same polished message instead of
// throwing raw out of the input handler.
function reportCfError(pi: ExtensionAPI, ctx: any, error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  ctx.ui.notify(`ConsensFlow error: ${message}`, "error");
  sendCfMessage(pi, `# ConsensFlow error\n\n${message}`, { error: message });
}

function helpText() {
  return `# ConsensFlow help

One agent at a time. Ask the lead in words — "spawn zeus as a reviewer, give
him our conversation, and ask what breaks on rollback" — and it composes the
run. The agent gets your brief, your task, and the session handoff when the
lead passes one.

The explicit form, if you want to drive it yourself:

\`\`\`text
/consensflow:cf @zeus What do you think about this approach?
/consensflow:cf @builder Make the minimal fix
\`\`\`

Add agents (shared across Pi and Claude Code, ${agentsPath(process.cwd())}):

\`\`\`text
/consensflow:presets                                    # list the curated presets
/consensflow:agents add zeus                      # add a preset
/consensflow:agents add zeus --name Deepreview    # preset backend, your own name -> @deepreview
/consensflow:agents add all                       # every preset
/consensflow:agents add --name Builder --kind codex --model gpt-5.6-sol --effort high \\
                                # fully custom, write-capable
\`\`\`

Admin commands:

- \`/consensflow:status\`
- \`/consensflow:doctor\`
- \`/consensflow:presets\`
- \`/consensflow:agents list|presets|add|show|remove|sync\`

Rules:

- Send to one agent at a time.
- One-shot: agents do not remember previous calls; each call re-sends the current session handoff.
- New agents are addressed with \`@name\` or \`/consensflow:cf @name …\`; no per-agent slash commands are registered.
- The current Pi session remains the lead and decides what to implement.
`;
}
