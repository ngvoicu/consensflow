import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

/**
 * What was said in a harness's own session — read, never written.
 *
 * `cf run` records every turn it makes, but a user who takes a conversation
 * over with `cf attach` is typing into the harness's own window, and those
 * turns leave no run directory. The lead was blind to them. The alternative
 * was reading the pane's screen, which is a picture: ANSI codes, wrapping, no
 * completion signal, and a marker you have to invent and then discover matches
 * your own echoed command. The harness's session file is the same conversation
 * as structured records.
 *
 * These are undocumented internal layouts. Two rules follow from that: read
 * only, never write — the store belongs to the harness, exactly as Claude
 * Code's settings.json belongs to the user — and every failure degrades to an
 * empty list. Catching up is a convenience; a harness that moves its files in
 * a patch release must cost the lead a "nothing to show", not a broken run.
 */
export async function harnessTurns(kind, sessionId, env = process.env) {
  if (!sessionId) return [];
  try {
    switch (kind) {
      case "codex":
        return await readJsonl(await findFile(codexRoot(env), (name) => name.includes(sessionId)), codexTurn);
      case "claude-code":
        return await readJsonl(await findFile(claudeRoot(env), (name) => name === `${sessionId}.jsonl`), claudeTurn);
      case "pi":
        return await readJsonl(await findFile(piRoot(env), (name) => name.includes(sessionId)), piTurn);
      case "opencode":
        return await readOpencode(env, sessionId);
      default:
        // image agents hold no conversation, and an unknown kind is not ours.
        return [];
    }
  } catch {
    return [];
  }
}

const home = (env) => env.HOME ?? env.USERPROFILE ?? os.homedir();
const codexRoot = (env) => path.join(env.CODEX_HOME ?? path.join(home(env), ".codex"), "sessions");
const claudeRoot = (env) => path.join(env.CLAUDE_CONFIG_DIR ?? path.join(home(env), ".claude"), "projects");
const piRoot = (env) => path.join(home(env), ".pi", "agent", "sessions");
const opencodeRoot = (env) =>
  path.join(env.XDG_DATA_HOME ?? path.join(home(env), ".local", "share"), "opencode", "storage");

/** Depth-first search for the one file whose name carries the session id. */
async function findFile(root, matches, depth = 6) {
  if (depth < 0) return null;
  let entries;
  try {
    entries = await fs.readdir(root, { withFileTypes: true });
  } catch {
    return null;
  }
  for (const entry of entries) {
    const full = path.join(root, entry.name);
    if (entry.isFile() && matches(entry.name)) return full;
    if (entry.isDirectory()) {
      const found = await findFile(full, matches, depth - 1);
      if (found !== null) return found;
    }
  }
  return null;
}

async function readJsonl(file, toTurn) {
  if (file === null) return [];
  const raw = await fs.readFile(file, "utf8");
  const turns = [];
  for (const line of raw.split("\n")) {
    if (line.trim().length === 0) continue;
    let parsed;
    try {
      parsed = JSON.parse(line);
    } catch {
      // A half-written last line is normal while a session is live.
      continue;
    }
    const turn = toTurn(parsed);
    if (turn !== null) turns.push(turn);
  }
  return turns;
}

function codexTurn(record) {
  if (record?.type !== "response_item") return null;
  const item = record.payload ?? record.item;
  return messageTurn(item?.role, item?.content);
}

function claudeTurn(record) {
  if (record?.type !== "user" && record?.type !== "assistant") return null;
  return messageTurn(record.message?.role ?? record.type, record.message?.content);
}

function piTurn(record) {
  if (record?.type !== "message") return null;
  return messageTurn(record.message?.role, record.message?.content);
}

/** Every harness spells a turn as a role plus content that is text or parts. */
function messageTurn(role, content) {
  if (role !== "user" && role !== "assistant") return null;
  const text = readable(flatten(content));
  return text.length === 0 ? null : { role, text };
}

/**
 * What a person would say was said.
 *
 * Two kinds of noise sit in these files under a `user` role. The harness's own
 * injected context — codex opens with `<recommended_plugins>` and
 * `<skills_instructions>` — is the environment talking to itself, and showing
 * it as "you asked" would be a lie. And our own packet wraps the question in
 * scaffolding the reader already knows; the question is the part they wanted.
 */
function readable(text) {
  if (text.startsWith("<")) return "";
  const marker = "## Message from the user";
  const at = text.indexOf(marker);
  if (at === -1) return text;
  const body = text.slice(at + marker.length).trim();
  // The packet closes with a formatting instruction that is ours, not theirs.
  const end = body.indexOf("\nRespond directly and conversationally");
  return (end === -1 ? body : body.slice(0, end)).trim();
}

function flatten(content) {
  if (typeof content === "string") return content.trim();
  if (!Array.isArray(content)) return "";
  return content
    .map((part) => (typeof part === "string" ? part : (part?.text ?? "")))
    .filter(Boolean)
    .join("\n")
    .trim();
}

/**
 * opencode keeps the turn and its text apart: `message/<session>/<msg>.json`
 * holds the role, `part/<msg>/*.json` holds what was said.
 */
async function readOpencode(env, sessionId) {
  const store = opencodeRoot(env);
  const dir = path.join(store, "message", sessionId);
  let names;
  try {
    names = await fs.readdir(dir);
  } catch {
    return [];
  }

  const messages = [];
  for (const name of names) {
    if (!name.endsWith(".json")) continue;
    try {
      messages.push(JSON.parse(await fs.readFile(path.join(dir, name), "utf8")));
    } catch {
      // skip a message we cannot read rather than losing the rest
    }
  }
  messages.sort((a, b) => (a?.time?.created ?? 0) - (b?.time?.created ?? 0));

  const turns = [];
  for (const message of messages) {
    if (message?.role !== "user" && message?.role !== "assistant") continue;
    const partDir = path.join(store, "part", String(message.id));
    let partNames = [];
    try {
      partNames = (await fs.readdir(partDir)).sort();
    } catch {
      continue;
    }
    const texts = [];
    for (const part of partNames) {
      try {
        const parsed = JSON.parse(await fs.readFile(path.join(partDir, part), "utf8"));
        if (parsed?.type === "text" && parsed.text) texts.push(String(parsed.text));
      } catch {
        // skip
      }
    }
    const text = texts.join("\n").trim();
    if (text.length > 0) turns.push({ role: message.role, text });
  }
  return turns;
}

/**
 * The session an opencode window just created — found, not told.
 *
 * opencode is the one harness that neither takes a session id at interactive
 * start nor prints the one it minted. Its store does know: every session file
 * records the directory it belongs to and when it was created. So the caller
 * spawns the window, remembers the clock, and asks the store afterwards for
 * the newest session in that directory born since. Read-only like everything
 * else in this file, and null rather than an error when nothing matches yet.
 */
export async function discoverOpencodeSession(cwd, since, env = process.env) {
  const root = path.join(opencodeRoot(env), "session");
  let projects;
  try {
    projects = await fs.readdir(root, { withFileTypes: true });
  } catch {
    return null;
  }
  let best = null;
  for (const project of projects) {
    if (!project.isDirectory()) continue;
    let names;
    try {
      names = await fs.readdir(path.join(root, project.name));
    } catch {
      continue;
    }
    for (const name of names) {
      if (!name.endsWith(".json")) continue;
      let session;
      try {
        session = JSON.parse(await fs.readFile(path.join(root, project.name, name), "utf8"));
      } catch {
        continue;
      }
      const created = session?.time?.created ?? 0;
      if (session?.directory !== cwd || created < since) continue;
      if (best === null || created > best.created) best = { id: session.id, created };
    }
  }
  return best?.id ?? null;
}
