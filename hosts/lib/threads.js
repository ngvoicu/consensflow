import crypto from "node:crypto";
import fs from "node:fs/promises";
import path from "node:path";
import { cfRoot, writeJsonAtomic } from "./state.js";

/**
 * Named conversations, per workspace.
 *
 * A `cf run` is a fresh process every time — the harnesses are launched with
 * their session machinery off. This is the one thing we remember so it need
 * not be: which harness session a named conversation belongs to. The harness
 * owns the session; we own the name and the id we hand back to it.
 *
 * Names exist because one agent can hold several conversations at once
 * ("ask ares in bubble-sky about xyz"), which is also what removes any need
 * for a lock: two conversations are two sessions and never collide.
 *
 * The store sits beside `runs/` under the one root, never inside the project.
 * It is an index, not the user's data — the transcripts in the run directories
 * are the record. So a store we cannot read degrades to "no conversations
 * yet" rather than breaking every consult.
 */
export function threadsPath(cwd) {
  return path.join(cfRoot(cwd), "threads.json");
}

export async function loadThreads(cwd) {
  try {
    const raw = await fs.readFile(threadsPath(cwd), "utf8");
    const parsed = JSON.parse(raw);
    // A JSON scalar or array would satisfy JSON.parse and then break every
    // caller that expects a name→record map.
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return parsed;
  } catch {
    // Missing is the common case; corrupt is the rare one. Both mean the same
    // thing to a caller: this workspace has no conversations we can resume.
    return {};
  }
}

export async function saveThread(cwd, name, record) {
  const threads = await loadThreads(cwd);
  threads[name] = record;
  await writeThreads(cwd, threads);
}

export async function removeThread(cwd, name) {
  const threads = await loadThreads(cwd);
  if (!(name in threads)) return;
  delete threads[name];
  await writeThreads(cwd, threads);
}

/** One atomic writer for the whole engine — tmp+rename, shared with state.js. */
async function writeThreads(cwd, threads) {
  await writeJsonAtomic(threadsPath(cwd), threads);
}

/**
 * The vocabulary a session name is drawn from.
 *
 * Concrete and everyday on purpose. Agents are named for gods; if a session
 * could be called `athena` too, "ask ares in athena" would read as two agents.
 * Keeping the two vocabularies disjoint makes the grammar unambiguous to a
 * reader and to the lead composing the command.
 */
const FIRST = [
  "amber", "bubble", "copper", "dusty", "ember", "frosty", "golden", "hazy",
  "ivory", "jade", "kelp", "lilac", "misty", "nutmeg", "olive", "pebble",
  "quartz", "rusty", "silver", "teal", "umber", "velvet", "willow", "yellow",
];

const SECOND = [
  "bloom", "brook", "cloud", "dune", "fern", "glade", "harbor", "island",
  "lagoon", "meadow", "moss", "orchard", "pine", "reef", "ridge", "sky",
  "spring", "thicket", "tide", "valley", "waves", "willows",
];

/** Every name the vocabulary can produce — the bound on how many can coexist. */
export function allSessionNames() {
  const names = [];
  for (const first of FIRST) for (const second of SECOND) names.push(`${first}-${second}`);
  return names;
}

/**
 * A fresh, sayable name for a conversation — the agent's name, then two words.
 *
 * `ilmarinen-quartz-valley` rather than `quartz-valley`: a conversation is
 * always somebody's, and the name is what a lead types into `cf catchup`, what
 * a pane tab carries, and what the user reads across a row of panes. Without
 * the agent in it, none of those say whose window they are, and a workspace
 * with four conversations is four two-word names to keep straight in your head.
 *
 * It also settles the ambiguity the disjoint vocabulary used to guard: "ask
 * ares in athena" could read as two agents, so the second word list is
 * deliberately everyday rather than mythological. Prefixing the owner makes
 * the grammar unambiguous by construction — `ares-bubble-sky` is one thing,
 * and obviously ares's.
 *
 * `taken` is the names already live here; `reserved` is the roster's agent
 * names, so a conversation can never be exactly an agent's name. Random rather
 * than sequential so two names side by side in a pane list are not mistaken
 * for a sequence.
 */
export function newSessionName(taken = [], reserved = [], agent = "") {
  const used = new Set([...taken, ...reserved]);
  const prefix = String(agent).trim().length > 0 ? `${String(agent).trim()}-` : "";
  // Try at random first: cheap, and avoids always handing out the same name
  // after a collision.
  for (let attempt = 0; attempt < 64; attempt += 1) {
    const name = `${prefix}${pick(FIRST)}-${pick(SECOND)}`;
    if (!used.has(name)) return name;
  }
  // Crowded workspace: fall back to a scan so a real answer is still found.
  const free = allSessionNames()
    .map((name) => `${prefix}${name}`)
    .filter((name) => !used.has(name));
  if (free.length > 0) return free[crypto.randomInt(free.length)];
  throw new Error(
    `no unused session name is left in this workspace (${used.size} taken) — end one with \`cf run --new\` or remove it`,
  );
}

function pick(list) {
  return list[crypto.randomInt(list.length)];
}

/**
 * The environment keys that identify a lead, most specific first.
 *
 * The harness's own session id wins over the pane it sits in: start a new
 * Claude Code session in the SAME cmux pane and the pane id has not changed,
 * so the pane would hand the new lead the previous one's conversations —
 * exactly what this is here to stop.
 */
const LEAD_KEYS = ["CLAUDE_CODE_SESSION_ID", "CMUX_SURFACE_ID", "ITERM_SESSION_ID", "TERM_SESSION_ID"];

/**
 * Who is asking — the lead session, not the machine and not the directory.
 *
 * A conversation belongs to the lead that started it. Without this, the most
 * recent conversation in a directory was everybody's: a brand-new Claude Code
 * session asking for a joke became turn 4 of whatever the previous lead had
 * been discussing there (live, 2026-08-24). Continuing only helps when the one
 * continuing is the one who was there.
 *
 * `null` means we could not tell, and a caller starting a consult must then
 * open a fresh conversation rather than join one. Two unidentified shells are
 * not the same lead, and treating them as one recreates the bug for precisely
 * the leads least able to notice it. The cost is the opposite direction —
 * a lead we cannot recognise never continues anything — which is the safe way
 * to be wrong.
 */
export function leadId(env = {}) {
  for (const key of LEAD_KEYS) {
    const value = env[key];
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}
