import crypto from "node:crypto";
import { realpathSync } from "node:fs";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { nowIso, slugify, stripMention } from "./utils.js";
import { isOrphanedPreset, syncAgentWithPreset } from "./presets.js";

// "image" is a backend-based kind (Codex Responses → gpt-image-2 via the Codex CLI login), not a
// CLI runner: it is handled upstream in cf.mjs, and buildRunnerInvocation keeps a loud backstop.
export const AGENT_KINDS = ["pi", "claude-code", "codex", "opencode", "image"];
export const SKILLS_POLICIES = ["default", "none", "explicit"];

// Older builds kept per-tool rosters below the shared home. Keep a one-time migration path so
// those users do not appear to lose agents when upgrading to the shared roster.
const LEGACY_AGENT_DIRS = ["consensflow-cc", "consensflow-pi"];

// Config home shared by both host tools (~/.consensflow; CONSENSFLOW_HOME overrides it — tests
// point it at a temp dir). Agent config and run artifacts all live directly under this
// home; there are no per-tool config roots.
export function configHome() {
  return process.env.CONSENSFLOW_HOME || path.join(os.homedir(), ".consensflow");
}

export function configRoot() {
  return configHome();
}

// Workspace artifacts (runs, session stash, pending prompt) live under the config home too,
// keyed by workspace path — ConsensFlow never creates a directory inside the project itself.
export function workspaceKey(cwd) {
  let resolved = path.resolve(cwd);
  // Canonicalize symlinks (e.g. /var vs /private/var on macOS) so every spelling of the same
  // workspace maps to one key, no matter which process computes it.
  try {
    resolved = realpathSync(resolved);
  } catch {}
  const hash = crypto.createHash("sha256").update(resolved).digest("hex").slice(0, 8);
  return `${slugify(path.basename(resolved)) || "workspace"}-${hash}`;
}

export function cfRoot(cwd) {
  return path.join(configRoot(), "workspaces", workspaceKey(cwd));
}

// Shared across both host tools so agents are defined once and usable from either.
export function agentsPath(_cwd) {
  return path.join(configHome(), "agents.json");
}

// The roster's name before the vocabulary settled (2026-08-21). A machine that
// still has one is read from it; the next write lands in agents.json.
export function legacyAgentsPath(_cwd) {
  return path.join(configHome(), "participants.json");
}

export function currentPath(cwd) {
  return path.join(cfRoot(cwd), "current.json");
}

export function runsRoot(cwd) {
  return path.join(cfRoot(cwd), "runs");
}

// Per-workspace stash of the live Claude Code session (transcript path etc.), maintained by the
// plugin hooks so cf.mjs can build a handoff — Bash subprocesses get no session env from the host.
export function sessionPath(cwd) {
  return path.join(cfRoot(cwd), "session.json");
}

export async function ensureCfDirs(cwd) {
  await fs.mkdir(configRoot(), { recursive: true });
  await fs.mkdir(cfRoot(cwd), { recursive: true });
  await fs.mkdir(runsRoot(cwd), { recursive: true });
}

async function readJsonIfExists(filePath) {
  try {
    const raw = await fs.readFile(filePath, "utf8");
    return JSON.parse(raw);
  } catch (error) {
    if (error && error.code === "ENOENT") return undefined;
    throw error;
  }
}

async function readJson(filePath, fallback) {
  const value = await readJsonIfExists(filePath);
  return value === undefined ? fallback : value;
}

export async function writeJsonAtomic(filePath, value) {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  const tmp = `${filePath}.tmp-${process.pid}-${Date.now()}`;
  await fs.writeFile(tmp, `${JSON.stringify(value, null, 2)}\n`, "utf8");
  await fs.rename(tmp, filePath);
}

export async function loadAgentsFile(cwd) {
  const file = await readJsonIfExists(agentsPath(cwd));
  if (file !== undefined) return normalizeAgentsFileShape(file);
  // The same roster under its old name, for a machine set up before the
  // rename. Reading it is enough: the next write lands in agents.json.
  const renamed = await readJsonIfExists(legacyAgentsPath(cwd));
  if (renamed !== undefined) return normalizeAgentsFileShape(renamed);
  const migrated = await migrateLegacyAgentsFile(cwd);
  return migrated ?? { schemaVersion: 1, agents: [] };
}

function normalizeAgentsFileShape(file) {
  if (!file || typeof file !== "object" || Array.isArray(file)) return { schemaVersion: 1, agents: [] };
  if (!Array.isArray(file.agents)) {
    // `participants` was this key's name until 2026-08-21.
    file.agents = Array.isArray(file.participants) ? file.participants : [];
  }
  delete file.participants;
  return file;
}

function legacyAgentsPaths() {
  return LEGACY_AGENT_DIRS.map((dir) => path.join(configHome(), dir, "agents.json"));
}

async function migrateLegacyAgentsFile(cwd) {
  const agents = [];
  for (const filePath of legacyAgentsPaths()) {
    const legacy = await readJsonIfExists(filePath);
    if (legacy && Array.isArray(legacy.agents)) agents.push(...legacy.agents);
  }
  if (agents.length === 0) return null;

  const byId = new Map();
  for (const raw of agents) {
    const agent = normalizeAgent(raw);
    if (!byId.has(agent.id)) byId.set(agent.id, agent);
  }
  const migrated = { schemaVersion: 1, agents: [...byId.values()] };
  assertUniqueAgents(migrated.agents);
  await writeJsonAtomic(agentsPath(cwd), migrated);
  return migrated;
}

export async function saveAgentsFile(cwd, file) {
  const normalized = {
    schemaVersion: 1,
    agents: file.agents.map((agent) => normalizeAgent(agent)),
  };
  assertUniqueAgents(normalized.agents);
  await writeJsonAtomic(agentsPath(cwd), normalized);
  return normalized;
}

export async function loadAgents(cwd) {
  return (await loadAgentsFile(cwd)).agents;
}

export async function getAgent(cwd, ref) {
  const id = slugify(stripMention(ref));
  const agents = await loadAgents(cwd);
  return agents.find((agent) => agent.id === id || slugify(agent.name) === id) ?? null;
}

export async function upsertAgent(cwd, input) {
  const file = await loadAgentsFile(cwd);
  const now = nowIso();
  const agent = normalizeAgent({ ...input, updatedAt: now, createdAt: input.createdAt ?? now });
  const index = file.agents.findIndex((entry) => entry.id === agent.id);
  if (index >= 0) {
    agent.createdAt = file.agents[index].createdAt ?? agent.createdAt;
    file.agents[index] = agent;
  } else {
    file.agents.push(agent);
  }
  await saveAgentsFile(cwd, file);
  return agent;
}

// Re-resolve every preset-backed agent against the current catalog, so a ConsensFlow
// update reaches agents that were added under an older one. Custom agents and
// entries whose preset has left the catalog are reported, never rewritten.
export async function syncAgentsWithPresets(cwd, { dryRun = false } = {}) {
  const file = await loadAgentsFile(cwd);
  const now = nowIso();
  const synced = [];
  file.agents = file.agents.map((entry) => {
    const { agent, changes } = syncAgentWithPreset(entry);
    if (changes.length === 0) return entry;
    synced.push({ id: agent.id, name: agent.name, changes });
    return { ...agent, updatedAt: now };
  });
  if (synced.length > 0 && !dryRun) await saveAgentsFile(cwd, file);
  return {
    synced,
    dryRun,
    total: file.agents.length,
    orphans: file.agents.filter(isOrphanedPreset).map((agent) => `@${agent.id}`),
  };
}

export async function removeAgent(cwd, ref) {
  const id = slugify(stripMention(ref));
  const file = await loadAgentsFile(cwd);
  const before = file.agents.length;
  file.agents = file.agents.filter((agent) => agent.id !== id && slugify(agent.name) !== id);
  await saveAgentsFile(cwd, file);
  return before !== file.agents.length;
}

export function normalizeAgent(input) {
  const name = String(input.name ?? input.id ?? "").trim();
  if (!name) throw new Error("Agent name is required");
  const id = slugify(input.id ?? name);
  const kind = String(input.kind ?? "pi");
  if (!AGENT_KINDS.includes(kind)) {
    throw new Error(`Unsupported agent kind '${kind}'. Expected one of: ${AGENT_KINDS.join(", ")}`);
  }

  const skillsPolicy = normalizeEnum(input.skillsPolicy ?? input.skills, SKILLS_POLICIES, "default", "skillsPolicy");

  const agent = {
    id,
    name,
    kind,
    skillsPolicy,
    createdAt: input.createdAt ?? nowIso(),
    updatedAt: input.updatedAt ?? nowIso(),
  };

  for (const key of ["model", "provider", "effort", "thinking", "harness", "cwd", "description", "preset"]) {
    if (input[key] !== undefined && input[key] !== true && String(input[key]).trim()) {
      agent[key] = String(input[key]).trim();
    }
  }

  const skillPaths = normalizeList(input.skillPaths ?? input.skillPath, []);
  if (skillPaths.length > 0) agent.skillPaths = skillPaths;

  if (input.maxTurns !== undefined) agent.maxTurns = Number(input.maxTurns);
  return agent;
}

function normalizeList(value, fallback) {
  if (Array.isArray(value)) return value.map((item) => String(item).trim()).filter(Boolean);
  if (typeof value === "string") return value.split(",").map((item) => item.trim()).filter(Boolean);
  return [...fallback];
}

function normalizeEnum(value, allowed, fallback, label) {
  const normalized = String(value ?? fallback).trim();
  if (!allowed.includes(normalized)) {
    throw new Error(`${label} must be one of: ${allowed.join(", ")}`);
  }
  return normalized;
}

// getAgent resolves @refs by id OR slugified name, so both must be unique across the
// roster — otherwise one agent's name slug could silently shadow another's id.
function assertUniqueAgents(agents) {
  const seen = new Map();
  for (const agent of agents) {
    for (const key of new Set([agent.id, slugify(agent.name)].filter(Boolean))) {
      if (seen.has(key)) throw new Error(`Agent '@${agent.id}' collides with '@${seen.get(key)}' on '${key}': ids and slugified names must be unique.`);
      seen.set(key, agent.id);
    }
  }
}

export async function loadCurrent(cwd) {
  return await readJson(currentPath(cwd), { schemaVersion: 1, latestRunId: undefined });
}

export async function saveCurrent(cwd, patch) {
  const current = await loadCurrent(cwd);
  const next = { ...current, ...patch, schemaVersion: 1, updatedAt: nowIso() };
  await writeJsonAtomic(currentPath(cwd), next);
  return next;
}

export async function recordLatestRun(cwd, result) {
  await saveCurrent(cwd, {
    latestRunId: result.runId,
    latestRunDir: result.runDir,
    latestAgentId: result.agent?.id,
    latestKind: result.kind,
  });
}

export async function loadSession(cwd) {
  return await readJson(sessionPath(cwd), { schemaVersion: 1 });
}

export async function saveSession(cwd, patch) {
  const session = await loadSession(cwd);
  const next = { ...session, ...patch, schemaVersion: 1, updatedAt: nowIso() };
  await writeJsonAtomic(sessionPath(cwd), next);
  return next;
}
