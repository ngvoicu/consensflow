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
 * A fresh, sayable name for a conversation in this workspace.
 *
 * `taken` is the names already live here; `reserved` is the roster's agent
 * names. Random rather than sequential so two names sitting next to each other
 * in a pane list are not mistaken for a sequence.
 */
export function newSessionName(taken = [], reserved = []) {
  const used = new Set([...taken, ...reserved]);
  // Try at random first: cheap, and avoids always handing out the same name
  // after a collision.
  for (let attempt = 0; attempt < 64; attempt += 1) {
    const name = `${pick(FIRST)}-${pick(SECOND)}`;
    if (!used.has(name)) return name;
  }
  // Crowded workspace: fall back to a scan so a real answer is still found.
  const free = allSessionNames().filter((name) => !used.has(name));
  if (free.length > 0) return free[crypto.randomInt(free.length)];
  throw new Error(
    `no unused session name is left in this workspace (${used.size} taken) — end one with \`cf run --new\` or remove it`,
  );
}

function pick(list) {
  return list[crypto.randomInt(list.length)];
}
