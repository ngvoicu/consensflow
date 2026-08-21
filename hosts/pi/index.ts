import fs from "node:fs/promises";
import path from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { serializeTranscript } from "../lib/handoff.js";
import { decodeChatGptAccountId, generateImage, imageFileToDataUrl, IMAGE_TRIGGER_DEFAULT, saveImagePng } from "../lib/image.js";
import { driftedParticipants, formatPresets, getPreset, listPresetIds, participantFromPreset } from "../lib/presets.js";
import {
  cfRoot,
  configHome,
  configRoot,
  ensureCfDirs,
  getParticipant,
  loadCurrent,
  loadParticipants,
  participantsPath,
  recordLatestRun,
  removeParticipant,
  runsRoot,
  syncParticipantsWithPresets,
  upsertParticipant,
} from "../lib/state.js";
import { createId, parseOptions, parseParticipantPrompt, slugify, tokenize } from "../lib/utils.js";
import { runNamedParticipant } from "../lib/workflows.js";
import { renderEvent } from "../lib/transcript-events.js";

const EXT = "consensflow";

export default async function consensflow(pi: ExtensionAPI) {
  pi.on("session_start", async (_event, ctx) => {
    const participants = await loadParticipants(ctx.cwd).catch(() => []);
    ctx.ui.setStatus(EXT, `CF ${participants.length} participant${participants.length === 1 ? "" : "s"}`);
  });

  pi.on("input", async (event, ctx) => {
    const parsed = await parseTypedPrompt(event.text, ctx);
    if (!parsed) return;
    try {
      await ensureCfDirs(ctx.cwd);
      await handleParticipantPrompt(parsed, ctx, pi, ctx.signal);
    } catch (error) {
      reportCfError(pi, ctx, error);
    }
    return { action: "handled" as const };
  });

  registerCoreCommands(pi);

  pi.registerTool({
    name: "cf_list_participants",
    label: "CF Participants",
    description: "List globally configured ConsensFlow named participants.",
    promptSnippet: "List named ConsensFlow participants available for one-at-a-time natural-language prompts.",
    parameters: Type.Object({}),
    async execute(_toolCallId, _params, _signal, _onUpdate, ctx) {
      const participants = await loadParticipants(ctx.cwd);
      return { content: [{ type: "text", text: formatParticipants(participants, ctx.cwd) }], details: { participants, ...storeDetails(ctx.cwd) } };
    },
  });

  pi.registerTool({
    name: "cf_run_participant",
    label: "CF Ask Participant",
    description: "Consult one named ConsensFlow participant as an advisor/helper. It receives the current session as a handoff plus your prompt, runs with its configured tools, and returns its answer. Use this freely and on your own initiative for second opinions, questions, implementation help, or design/code critique — you do not need the user's permission to consult. But do NOT apply, merge, commit, adopt, or otherwise act on what it returns — neither its advice nor a write-capable participant's file changes — without first showing the user (a summary plus your recommendation) and getting their approval, unless the user has already told you to proceed.",
    promptSnippet: "Consult one named ConsensFlow participant as an advisor/helper (asking is free — no user permission needed). Then report its answer to the user and get approval before acting on it: never apply a participant's advice or changes unprompted unless the user already said to proceed.",
    parameters: Type.Object({
      participant: Type.String({ description: "Participant name or @mention, e.g. @zeus" }),
      prompt: Type.String({ description: "Natural-language request for that participant" }),
      context: Type.Optional(Type.String({ description: "Optional focused note/brief added on top of the auto-included session handoff." })),
      includeHandoff: Type.Optional(Type.Boolean({ description: "Attach the current session transcript as context. Defaults to true." })),
      images: Type.Optional(Type.Array(Type.String(), { description: "Image-participant only (kind=image): file paths to reference images (.png/.jpg/.jpeg/.webp/.gif) for gpt-image-2 to edit/condition on. Ignored by text participants." })),
    }),
    async execute(_toolCallId, params, signal, onUpdate, ctx) {
      const participant = await getParticipant(ctx.cwd, params.participant);
      if (!participant) throw new Error(`Unknown participant: ${params.participant}`);
      if (participant.kind === "image") {
        onUpdate?.({ content: [{ type: "text", text: `Generating image with @${participant.id} (gpt-image-2)...` }] });
        const r = await generateImageArtifact(ctx, participant, params.prompt, signal, params.images);
        return {
          content: [
            { type: "text", text: imageSummary(participant, r) },
            { type: "image", data: r.base64, mimeType: r.mimeType },
          ],
          details: { runId: r.runId, savedPath: r.savedPath, revisedPrompt: r.revisedPrompt, participant: { id: participant.id, kind: participant.kind } },
        };
      }
      onUpdate?.({ content: [{ type: "text", text: `Asking @${participant.id}...` }] });
      const includeHandoff = params.includeHandoff ?? true;
      const handoff = includeHandoff ? collectHandoff(ctx) : "";
      let sawDelta = false;
      const result = await runNamedParticipant({
        cwd: ctx.cwd,
        participantRef: participant,
        kind: "ask",
        task: params.prompt,
        handoff,
        extraContext: params.context,
        signal,
        // Stream the participant's normalized thinking / tool calls / answer into the Pi UI as it
        // arrives — the pi analog of cc's always-on streaming (foreground-incremental observability).
        onEvent: (event: any) => {
          if (event.kind === "delta") { sawDelta = true; if (event.text) onUpdate?.({ content: [{ type: "text", text: event.text }] }); return; } // pi reasoning/text, flowing like its own UI
          // Once deltas have streamed, the message_end thinking/text blocks are redundant — skip them.
          if (sawDelta && (event.kind === "thinking" || event.kind === "text")) return;
          const line = renderEvent(event);
          if (line) onUpdate?.({ content: [{ type: "text", text: line }] });
        },
      });
      result.handoffSummary = summarizeHandoff(handoff, includeHandoff);
      return { content: [{ type: "text", text: renderRunResult(result) }], details: result };
    },
  });

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
    description: "ConsensFlow: manage named participants or send one prompt to one participant",
    toCfArgs: (args) => args,
  },
  {
    name: "consensflow:status",
    description: "ConsensFlow: show configured participants and the latest run",
    toCfArgs: () => "status",
  },
  {
    name: "consensflow:doctor",
    description: "ConsensFlow: check which engine CLIs are installed and working",
    toCfArgs: () => "doctor",
  },
  {
    name: "consensflow:presets",
    description: "ConsensFlow: list the curated participant presets",
    toCfArgs: () => "participants presets",
  },
  {
    name: "consensflow:participants",
    description: "ConsensFlow: list or manage your participants (add, show, remove, presets)",
    toCfArgs: (args) => prefixedArgs("participants", args),
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

    const known = await knownParticipantKeys(ctx.cwd);
    const commandName = tokens[0]?.toLowerCase();
    if (!CF_SUBCOMMANDS.has(commandName)) {
      const directPrompt = parseRunPrompt(tokens, known);
      if (directPrompt) return await handleParticipantPrompt(directPrompt, ctx, pi, ctx.signal);
    }

    const command = tokens.shift() ?? "status";
    switch (command) {
      case "status":
      case "state":
        return await handleStatus(ctx, pi);
      case "doctor":
        return await handleDoctor(ctx, pi);
      case "participants":
      case "participant":
        return await handleParticipants(tokens, ctx, pi);
      case "run":
      case "ask":
      case "to": {
        const parsed = parseRunPrompt(tokens, known);
        if (!parsed) throw new Error("Usage: /consensflow:cf @name <prompt> or /consensflow:cf ask @name <prompt>");
        return await handleParticipantPrompt(parsed, ctx, pi, ctx.signal);
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
  const participants = await loadParticipants(ctx.cwd);
  const current = await loadCurrent(ctx.cwd);
  const markdown = [
    "# ConsensFlow status",
    "",
    `ConsensFlow home: ${configHome()}`,
    `Participants file: ${participantsPath(ctx.cwd)}`,
    `Artifact root for this workspace: ${cfRoot(ctx.cwd)}`,
    `Participants: ${participants.length}${driftNote(participants)}`,
    `Latest run: ${current.latestRunId ?? "none"}`,
    "",
    formatParticipants(participants, ctx.cwd),
  ].join("\n");
  sendCfMessage(pi, markdown, { participants, current, ...storeDetails(ctx.cwd), artifactRoot: cfRoot(ctx.cwd) });
}

async function handleDoctor(ctx: any, pi: ExtensionAPI) {
  const KIND_BINARY: Record<string, string> = { pi: "pi", "claude-code": "claude", codex: "codex", opencode: "opencode" };
  const binaries = ["pi", "claude", "codex", "opencode"];
  const participants = (await loadParticipants(ctx.cwd).catch(() => [])) as any[];
  const neededBy: Record<string, string[]> = {};
  for (const p of participants) {
    const binary = KIND_BINARY[p.kind];
    if (binary) (neededBy[binary] ??= []).push(`@${p.id}`);
  }
  const rows = [];
  for (const binary of binaries) {
    const result = await pi.exec(binary, ["--version"], { timeout: 5000 });
    rows.push({ binary, ok: result.code === 0, output: (result.stdout || result.stderr || "").trim(), neededBy: neededBy[binary] ?? [] });
  }
  const imageParticipants = participants.filter((p) => p.kind === "image").map((p) => `@${p.id}`);
  const missing = rows.filter((row) => !row.ok && row.neededBy.length > 0);
  const lines = [
    "# ConsensFlow doctor",
    "",
    `ConsensFlow home: ${configHome()}`,
    `Participants file: ${participantsPath(ctx.cwd)}`,
    "",
    ...rows.map((row) => {
      const need = row.neededBy.length > 0 ? ` — needed by ${row.neededBy.join(", ")}` : " — not used by any participant";
      return `- ${row.ok ? "✓" : "✗"} ${row.binary}: ${row.output || "not available"}${need}`;
    }),
  ];
  if (imageParticipants.length > 0) {
    lines.push("", `- image participants (${imageParticipants.join(", ")}) need an \`openai-codex\` login (\`/login\` → ChatGPT Plus/Pro), not a CLI binary.`);
  }
  if (missing.length > 0) {
    lines.push("", "Missing engines that configured participants need:", ...missing.map((row) => `  - ${row.binary} (needed by ${row.neededBy.join(", ")})`));
  }
  sendCfMessage(pi, lines.join("\n"), { rows, imageParticipants, ...storeDetails(ctx.cwd) });
}

async function handleParticipants(tokens: string[], ctx: any, pi: ExtensionAPI) {
  const sub = tokens.shift() ?? "list";
  if (sub === "list") {
    const participants = await loadParticipants(ctx.cwd);
    return sendCfMessage(pi, formatParticipants(participants, ctx.cwd), { participants, ...storeDetails(ctx.cwd) });
  }
  if (sub === "presets" || sub === "preset") {
    return sendCfMessage(pi, formatPresets(), { presets: listPresetIds() });
  }
  if (sub === "sync") {
    const parsed = parseOptions(tokens);
    const result = await syncParticipantsWithPresets(ctx.cwd, { dryRun: Boolean(parsed.flags["dry-run"]) });
    if (result.synced.length > 0 && !result.dryRun) ctx.ui.notify(`Synced ${result.synced.length} participant${result.synced.length === 1 ? "" : "s"} with the catalog`, "info");
    return sendCfMessage(pi, formatSync(result), { ...result, ...storeDetails(ctx.cwd) });
  }

  if (sub === "show") {
    const ref = tokens[0];
    if (!ref) throw new Error("Usage: /consensflow:participants show @name");
    const participant = await getParticipant(ctx.cwd, ref);
    if (!participant) throw new Error(`Unknown participant: ${ref}`);
    return sendCfMessage(pi, `# ${participant.name}\n\n\`\`\`json\n${JSON.stringify(participant, null, 2)}\n\`\`\``, { participant });
  }
  if (sub === "remove" || sub === "rm") {
    const ref = tokens[0];
    if (!ref) throw new Error("Usage: /consensflow:participants remove @name");
    const removed = await removeParticipant(ctx.cwd, ref);
    ctx.ui.notify(removed ? `Removed ${ref}` : `No participant matched ${ref}`, removed ? "info" : "warning");
    return sendCfMessage(pi, removed ? `Removed ${ref}.` : `No participant matched ${ref}.`, { removed, ref });
  }
  if (sub === "add") {
    const parsed = parseOptions(tokens);
    const presetRef = parsed.positional[0];

    // Add every preset at once.
    if (presetRef === "all") {
      // `--name`/`--id` would make every preset derive the same id and overwrite each other (saving
      // one participant while reporting "Saved 24"). Only allow flags that apply uniformly to a bulk add.
      assertAllowedFlags(parsed.flags, ["cwd", "description"], "preset add all");
      const participants = [];
      for (const presetId of listPresetIds()) {
        participants.push(await upsertParticipant(ctx.cwd, participantFromPreset(presetId, presetOverrides(parsed.flags))));
      }
      ctx.ui.notify(`Saved ${participants.length} ConsensFlow participants`, "info");
      return sendCfMessage(pi, `Saved presets in ${participantsPath(ctx.cwd)}.\n\n${participants.map(formatParticipantLine).join("\n")}`, { participants, ...storeDetails(ctx.cwd) });
    }

    // Preset path: positional names a known preset; --name optionally renames it.
    if (presetRef && getPreset(presetRef)) {
      assertPresetOverrideFlags(parsed.flags);
      const participant = await upsertParticipant(ctx.cwd, participantFromPreset(presetRef, presetOverrides(parsed.flags)));
      ctx.ui.notify(`Saved @${participant.id}`, "info");
      const from = participant.preset && participant.preset !== participant.id ? ` from preset \`${participant.preset}\`` : "";
      return sendCfMessage(pi, `Saved participant @${participant.id}${from} in ${participantsPath(ctx.cwd)}.\n\n${formatParticipantLine(participant)}`, { participant, ...storeDetails(ctx.cwd) });
    }

    // Custom path: explicit custom intent via --name or any backend flag. A positional serves as the name.
    if (stringFlag(parsed.flags.name) !== undefined || hasCustomShape(parsed.flags)) {
      assertCustomAddFlags(parsed.flags);
      const name = stringFlag(parsed.flags.name) ?? presetRef;
      if (!name) throw new Error("Custom participant needs a name: /consensflow:participants add --name <name> --kind <kind> --model <model> ...");
      const participant = await upsertParticipant(ctx.cwd, customParticipantInput(name, parsed.flags));
      ctx.ui.notify(`Saved @${participant.id}`, "info");
      return sendCfMessage(pi, `Saved custom participant @${participant.id} in ${participantsPath(ctx.cwd)}.\n\n${formatParticipantLine(participant)}`, { participant, ...storeDetails(ctx.cwd) });
    }

    if (presetRef) {
      throw new Error(`Unknown preset: ${presetRef}\n\nPresets: ${listPresetIds().join(", ")} (rename any with --name).\n\nOr create a custom participant:\n  /consensflow:participants add --name <name> --kind <pi|claude-code|codex|opencode|image> --model <model> [--effort <e>]`);
    }
    throw new Error(addUsage());
  }
  throw new Error("Usage: /consensflow:participants list|presets|add|show|remove|sync");
}

async function handleParticipantPrompt(parsed: ParticipantPrompt, ctx: any, pi: ExtensionAPI, signal?: AbortSignal) {
  if (parsed.error) throw new Error(parsed.error);
  const participant = await getParticipant(ctx.cwd, parsed.participant);
  if (!participant) throw new Error(`Unknown participant: @${parsed.participant}`);
  if (participant.kind === "image") return await runImageParticipant(participant, parsed.prompt, ctx, pi, signal, parsed.images);
  ctx.ui.notify(`Asking @${participant.id}...`, "info");
  const includeHandoff = parsed.includeHandoff ?? true;
  const handoff = includeHandoff ? collectHandoff(ctx) : "";
  const result = await runNamedParticipant({
    cwd: ctx.cwd,
    participantRef: participant,
    kind: "ask",
    task: parsed.prompt,
    extraContext: parsed.context,
    handoff,
    signal,
    // Direct @mention and /consensflow:cf runs should be visible in the main session too, not
    // just as a final answer after a long child process. Stream the normalized trail as
    // lightweight custom messages; handoff serialization ignores these stream crumbs and keeps
    // only the final participant reply for cross-pollination.
    onEvent: (event: any) => sendCfStreamEvent(pi, participant, event),
  });
  result.handoffSummary = summarizeHandoff(handoff, includeHandoff);
  // Record the prompt in details so later participants' handoffs can reconstruct this exchange
  // (the @mention input was "handled" and is never stored as a normal session message).
  sendCfMessage(pi, renderRunResult(result), { ...result, prompt: parsed.prompt });
}

// --- Image participants (kind: "image") ---------------------------------
// Image generation doesn't fit the text-CLI runner: it calls the Codex Responses
// backend (gpt-image-2) over HTTP — reusing the openai-codex login — and returns
// an image. Handled here, not in runners.js, because it needs ctx.modelRegistry.
// The image model gets the prompt verbatim (no packet/handoff).
async function generateImageArtifact(ctx: any, participant: any, prompt: string, signal?: AbortSignal, imagePaths: string[] = []) {
  const token = await ctx?.modelRegistry?.getApiKeyForProvider?.("openai-codex");
  if (!token) {
    throw new Error("No openai-codex login found. Run /login and pick ChatGPT Plus/Pro (Codex) to use image participants.");
  }
  const accountId = decodeChatGptAccountId(token);
  await ensureCfDirs(ctx.cwd);
  const runId = createId("image");
  const runDir = path.join(runsRoot(ctx.cwd), runId);
  await fs.mkdir(runDir, { recursive: true });
  const triggerModel = participant.model || IMAGE_TRIGGER_DEFAULT;
  // Optional reference images become input_image parts so gpt-image-2 can edit/condition on them.
  const images = await Promise.all((imagePaths ?? []).map((p) => imageFileToDataUrl(p)));
  const image = await generateImage({ token, accountId, prompt, triggerModel, images, signal });
  const savedPath = await saveImagePng(image.base64, runDir, "image.png");
  await fs.writeFile(
    path.join(runDir, "result.json"),
    `${JSON.stringify({ runId, savedPath, triggerModel, backend: "gpt-image-2", referenceImages: imagePaths ?? [], revisedPrompt: image.revisedPrompt, responseId: image.responseId, participant: { id: participant.id, kind: participant.kind } }, null, 2)}\n`,
    "utf8",
  );
  await recordLatestRun(ctx.cwd, { runId, runDir, participant, kind: "image" });
  return { runId, runDir, savedPath, mimeType: "image/png", base64: image.base64, revisedPrompt: image.revisedPrompt, referenceImages: imagePaths ?? [] };
}

function imageSummary(participant: any, r: { savedPath: string; revisedPrompt?: string; referenceImages?: string[] }) {
  return [
    `# @${participant.id}`,
    "",
    "Generated an image with **gpt-image-2** (via your openai-codex login).",
    r.referenceImages?.length ? `Reference image(s): ${r.referenceImages.join(", ")}` : undefined,
    r.revisedPrompt ? `Revised prompt: ${r.revisedPrompt}` : undefined,
    `Saved: ${r.savedPath}`,
  ]
    .filter(Boolean)
    .join("\n");
}

async function runImageParticipant(participant: any, prompt: string, ctx: any, pi: ExtensionAPI, signal?: AbortSignal, imagePaths: string[] = []) {
  ctx.ui.notify(`Generating image with @${participant.id} (gpt-image-2)...`, "info");
  const r = await generateImageArtifact(ctx, participant, prompt, signal, imagePaths);
  pi.sendMessage({
    customType: EXT,
    content: [
      { type: "text", text: imageSummary(participant, r) },
      { type: "image", data: r.base64, mimeType: r.mimeType },
    ],
    display: true,
    details: { runId: r.runId, runDir: r.runDir, savedPath: r.savedPath, revisedPrompt: r.revisedPrompt, participant, prompt, kind: "image" },
  });
}

// Pull the current resolved session transcript on-demand from the read-only session manager and
// serialize it for the participant handoff. On-demand (not cached) so it stays correct across
// fork / tree navigation / session switch. Degrades to "" if unavailable.
function collectHandoff(ctx: any): string {
  try {
    const sessionManager = ctx?.sessionManager;
    if (!sessionManager || typeof sessionManager.getBranch !== "function") return "";
    return serializeTranscript(sessionManager.getBranch()); // 48 KB default + CONSENSFLOW_HANDOFF_MAX_BYTES env, from handoff.js
  } catch {
    return "";
  }
}

type ParticipantPrompt =
  | { participant: string; prompt: string; error?: undefined; context?: string; includeHandoff?: boolean; images?: string[] }
  | { participant?: undefined; prompt?: undefined; error: string };

// Tokenize a typed line and decide whether it addresses one participant. When the line contains
// any `@token`, load the configured participants so a single non-leading mention (`hi @zeus`)
// routes only when it names a real participant — a stray `@types/node` is left for the lead.
async function parseTypedPrompt(text: string, ctx: any): Promise<ParticipantPrompt | null> {
  const trimmed = String(text ?? "").trim();
  if (!trimmed || trimmed.startsWith("/")) return null;
  const tokens = tokenize(trimmed);
  if (!tokens.some((token) => token.startsWith("@"))) return null;
  return parseRunPrompt(tokens, await knownParticipantKeys(ctx.cwd));
}

const CF_SUBCOMMANDS = new Set(["status", "state", "doctor", "participants", "participant", "run", "ask", "to", "help"]);

function parseRunPrompt(tokens: string[], known: Set<string>): ParticipantPrompt | null {
  const parsed = parseRunOptions(tokens);
  const prompt = parseParticipantPrompt(parsed.positional, known) as ParticipantPrompt | null;
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

// Slugified ids + names of every configured participant, matching getParticipant's resolution.
async function knownParticipantKeys(cwd: string): Promise<Set<string>> {
  const participants = await loadParticipants(cwd).catch(() => []);
  return new Set((participants as any[]).flatMap((p) => [p.id, slugify(p.name)]).filter(Boolean));
}

const PRESET_OVERRIDE_FLAGS = ["name", "id", "cwd", "description"];
const CUSTOM_ADD_FLAGS = ["name", "id", "kind", "model", "provider", "effort", "thinking", "skills", "skillsPolicy", "agent", "cwd", "maxTurns", "description"];
const CUSTOM_SHAPE_FLAGS = ["kind", "model", "provider", "effort", "thinking", "skills", "skillsPolicy", "agent", "maxTurns"];

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

function customParticipantInput(name: string, flags: Record<string, unknown>) {
  return {
    name,
    id: flags.id,
    kind: flags.kind,
    model: flags.model,
    provider: flags.provider,
    effort: flags.effort,
    thinking: flags.thinking,
    skillsPolicy: flags.skills ?? flags.skillsPolicy,
    agent: flags.agent,
    cwd: flags.cwd,
    maxTurns: flags.maxTurns,
    description: flags.description,
  };
}

function addUsage() {
  return [
    "Usage:",
    "  /consensflow:participants add <preset> [--name <name>]        # from a preset, optionally renamed",
    "  /consensflow:participants add all                              # every preset",
    "  /consensflow:participants add --name <name> --kind <pi|claude-code|codex|opencode|image> --model <model> [--effort <e>] [--thinking <t>] [--cwd <subdir>]",
    "",
    `Presets: ${listPresetIds().join(", ")}`,
  ].join("\n");
}

function storeDetails(cwd: string) {
  return { configHome: configHome(), participantsPath: participantsPath(cwd), toolArtifactRoot: configRoot() };
}

function formatParticipants(participants: any[], cwd = process.cwd()) {
  if (participants.length === 0) {
    return [
      "# ConsensFlow participants",
      "",
      `Participants file: ${participantsPath(cwd)}`,
      "",
      "No participants configured yet.",
      "",
      "Create participants:",
      "```text",
      "/consensflow:presets                                    # list the curated presets",
      "/consensflow:participants add zeus                      # add a preset",
      "/consensflow:participants add zeus --name Deepreview    # preset backend, custom name",
      "/consensflow:participants add all                       # every preset",
      "/consensflow:participants add --name Builder --kind codex --model gpt-5.6-sol",
      "```",
    ].join("\n");
  }
  const drift = driftNote(participants);
  return [
    "# ConsensFlow participants",
    "",
    `Participants file: ${participantsPath(cwd)}`,
    ...(drift ? [`Catalog:${drift}`] : []),
    "",
    ...participants.map(formatParticipantLine),
  ].join("\n");
}

// One-line "you are behind the catalog" hint, appended wherever the roster is summarised.
function driftNote(participants: any[]) {
  const drifted = driftedParticipants(participants);
  if (drifted.length === 0) return "";
  return `  (${drifted.length} behind the catalog — run \`/consensflow:participants sync\`)`;
}

function formatSync(result: any) {
  const lines = ["# ConsensFlow participants sync", ""];
  if (result.synced.length === 0) {
    lines.push(`All ${result.total} participants already match the catalog.`);
  } else {
    for (const entry of result.synced) {
      for (const change of entry.changes) {
        // Descriptions are long; report them as a fact rather than a diff.
        lines.push(change.field === "description" ? `@${entry.id}  description updated` : `@${entry.id}  ${change.field}  ${change.from ?? "(none)"} → ${change.to ?? "(none)"}`);
      }
    }
    lines.push("");
    lines.push(`${result.dryRun ? "Would sync" : "Synced"} ${result.synced.length} participant${result.synced.length === 1 ? "" : "s"} (${result.total - result.synced.length} already current).`);
  }
  if (result.orphans.length > 0) lines.push("", `Left pinned (preset no longer in the catalog): ${result.orphans.join(", ")}`);
  return lines.join("\n");
}

function formatParticipantLine(p: any) {
  const model = p.model ? ` model=${p.model}` : "";
  const effort = p.effort ? ` effort=${p.effort}` : p.thinking ? ` thinking=${p.thinking}` : "";
  const cwd = p.cwd ? ` cwd=${p.cwd}` : "";
  const skills = p.kind === "pi" ? ` skills=${p.skillsPolicy ?? "default"}` : "";
  const preset = p.preset ? ` preset=${p.preset}` : "";
  const head = `- @${p.id} (${p.kind}${model}${effort}${cwd}${skills}${preset})`;
  return p.description ? `${head}\n    ${p.description}` : head;
}

// The run output reports what context rode along: a silently-empty handoff looks identical to a
// full one from the participant's answer alone.
function summarizeHandoff(handoff: string, included: boolean) {
  if (!included) return "skipped (includeHandoff=false)";
  if (!String(handoff ?? "").trim()) return "empty — no session history to hand off";
  return `attached (${Math.max(1, Math.round(Buffer.byteLength(handoff, "utf8") / 1024))} KB)`;
}

// Just the answer on a clean run. Diagnostics appear only when they matter: the run failed or the
// handoff was unexpectedly empty. Every participant now runs read-write, so the inspect-your-repo
// nudge always shows. Full metadata stays in result.json and the message details.
function renderRunResult(result: any) {
  const lines = [`# @${result.participant.id}`];
  if (result.exitCode !== 0) {
    lines.push("", `Run failed: exit ${result.exitCode} — artifacts: ${result.runDir}`);
  }
  if (result.handoffSummary?.startsWith("empty")) lines.push("", `Handoff: ${result.handoffSummary}`);
  lines.push("", "> Full-permission run: this participant ran unsandboxed — it could edit any file, run any command, and reach the network. Inspect what changed (e.g. `git status` / `git diff`) before keeping or building on it.");
  lines.push("", result.output);
  return lines.join("\n");
}

function sendCfMessage(pi: ExtensionAPI, content: string, details?: any) {
  pi.sendMessage({ customType: EXT, content, display: true, details });
}

function sendCfStreamEvent(pi: ExtensionAPI, participant: any, event: any) {
  const line = renderEvent(event);
  if (!line) return;
  sendCfMessage(pi, line, { streamEvent: true, participant: { id: participant.id, kind: participant.kind } });
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

Natural-language prompts to one participant at a time. Each participant gets the current
session as a handoff plus your prompt, and answers conversationally.

Ask a participant:

\`\`\`text
@zeus What do you think about this approach?                          # bare mention
/consensflow:cf @zeus What do you think about this approach?          # explicit router
/consensflow:cf @builder Make the minimal fix  # per-call write
\`\`\`

Add participants (shared across Pi and Claude Code, ${participantsPath(process.cwd())}):

\`\`\`text
/consensflow:presets                                    # list the curated presets
/consensflow:participants add zeus                      # add a preset
/consensflow:participants add zeus --name Deepreview    # preset backend, your own name -> @deepreview
/consensflow:participants add all                       # every preset
/consensflow:participants add --name Builder --kind codex --model gpt-5.6-sol --effort high \\
                                # fully custom, write-capable
\`\`\`

Admin commands:

- \`/consensflow:status\`
- \`/consensflow:doctor\`
- \`/consensflow:presets\`
- \`/consensflow:participants list|presets|add|show|remove|sync\`

Rules:

- Send to one participant at a time.
- One-shot: participants do not remember previous calls; each call re-sends the current session handoff.
- New participants are addressed with \`@name\` or \`/consensflow:cf @name …\`; no per-participant slash commands are registered.
- The current Pi session remains the lead and decides what to implement.
`;
}
