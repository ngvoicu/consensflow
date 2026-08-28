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
      case "kimi":
        return await readKimi(env, sessionId);
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
const kimiRoot = (env) => path.join(env.KIMI_CODE_HOME ?? path.join(home(env), ".kimi-code"), "sessions");
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

/** Like findFile, but the session id names a DIRECTORY — which is kimi's shape. */
async function findDir(root, name, depth = 4) {
  if (depth < 0) return null;
  let entries;
  try {
    entries = await fs.readdir(root, { withFileTypes: true });
  } catch {
    return null;
  }
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    if (entry.name === name) return path.join(root, entry.name);
    const found = await findDir(path.join(root, entry.name), name, depth - 1);
    if (found !== null) return found;
  }
  return null;
}

/**
 * Kimi Code keeps an event log, not a message list.
 *
 * `sessions/wd_<name>_<hash>/<sessionId>/agents/main/wire.jsonl` records what
 * the loop did: `turn.prompt` is what the user sent, and an answer arrives as
 * a run of `content.part` events carrying `text` (and `think`, which is the
 * agent reasoning to itself and not part of what was said).
 *
 * The parts of one answer MUST be joined by their `turnId`. A one-word reply
 * is a single part and a real one is many — counting each as a turn would
 * inflate the number that `cf catchup --unread` and `--wait` both key on.
 *
 * `session_index.jsonl` maps ids to directories, but it is a cache beside the
 * store rather than the store: the tree is what the harness actually writes.
 */
async function readKimi(env, sessionId) {
  const dir = await findDir(kimiRoot(env), sessionId);
  if (dir === null) return [];
  const raw = await fs.readFile(path.join(dir, "agents", "main", "wire.jsonl"), "utf8");

  const turns = [];
  let answer = null;
  const flush = () => {
    if (answer === null) return;
    const text = readable(answer.parts.join("").trim(), "assistant");
    if (text.length > 0) turns.push({ role: "assistant", text });
    answer = null;
  };

  for (const line of raw.split("\n")) {
    if (line.trim().length === 0) continue;
    let record;
    try {
      record = JSON.parse(line);
    } catch {
      continue;
    }

    if (record?.type === "turn.prompt" && record.origin?.kind === "user") {
      flush();
      const text = readable(flatten(record.input), "user");
      if (text.length > 0) turns.push({ role: "user", text });
      continue;
    }

    const event = record?.type === "context.append_loop_event" ? record.event : null;
    if (event?.type !== "content.part" || event.part?.type !== "text") continue;
    if (answer !== null && answer.turnId !== event.turnId) flush();
    if (answer === null) answer = { turnId: event.turnId, parts: [] };
    answer.parts.push(String(event.part.text ?? ""));
  }
  flush();
  return turns;
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
  const text = readable(flatten(content), role);
  return text.length === 0 ? null : { role, text };
}

/**
 * What a person would say was said.
 *
 * Two kinds of noise sit in these files, and — as the paragraph below always
 * said — both arrive under a `user` role. The harness's own injected context
 * (codex opens with `<recommended_plugins>` and `<skills_instructions>`) is
 * the environment talking to itself, and showing it as "you asked" would be a
 * lie. And our own packet wraps the question in scaffolding the reader
 * already knows; the question is the part they wanted.
 *
 * An ANSWER passes through untouched. It used to be filtered too, and the
 * filter was `startsWith("<")` — so an agent that opened with markup had its
 * whole answer dropped from every catchup, leaving the question standing
 * alone. Our own packet asks agents to "return only the requested output",
 * which is precisely how one asked for HTML replies (live, 2026-08-27).
 */
function readable(text, role) {
  if (role !== "user") return text;
  const body = withoutInjectedBlocks(text);
  const marker = "## Message from the user";
  const at = body.indexOf(marker);
  if (at === -1) return body;
  const question = body.slice(at + marker.length).trim();
  // The packet closes with a formatting instruction that is ours, not theirs.
  const end = question.indexOf("\nRespond directly and conversationally");
  return (end === -1 ? question : question.slice(0, end)).trim();
}

/**
 * Injected blocks off the front, whatever a person wrote left standing.
 *
 * A COMPLETE `<tag>…</tag>` is what an environment injects — verified against
 * a real codex rollout on 2026-08-27, closing tag and all. A lone opening tag
 * is somebody talking: `<div> tags are escaping wrong` is a question, and
 * `<!doctype html>` is an answer. Repeated, because one turn can carry more
 * than one block. If a harness ever injects a block it does not close, this
 * shows it rather than hiding it — the harmless direction for a reader whose
 * job is to lose nothing.
 */
const INJECTED_BLOCK = /^\s*<([a-z][a-z0-9_-]*)>[\s\S]*?<\/\1>\s*/i;

function withoutInjectedBlocks(text) {
  let rest = text;
  while (INJECTED_BLOCK.test(rest)) rest = rest.replace(INJECTED_BLOCK, "");
  return rest.trim();
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
 * opencode's store moved into SQLite, and the files we used to read froze.
 *
 * `storage/session|message|part` stopped being written on 2026-01-06 — there
 * is a `migration` marker beside them from the day before — and everything
 * since lives in `opencode.db`. The file reader kept passing its tests against
 * fixtures in the old shape while returning nothing for any real conversation,
 * which is the quiet way a reader dies: green tests over a dead format. Found
 * by opening a live opencode and looking for the turn (2026-08-24).
 *
 * Database first, old files second — a machine that has not run opencode since
 * January still reads correctly, and every failure degrades to empty.
 */
function opencodeDb(env) {
  return path.join(
    env.XDG_DATA_HOME ?? path.join(home(env), ".local", "share"),
    "opencode",
    "opencode.db",
  );
}

/**
 * Read-only, opened and closed per call. This is somebody else's live database
 * with a write-ahead log beside it: take no lock we do not need, hold nothing
 * open between reads, and never write.
 */
async function withOpencodeDb(env, fn) {
  const file = opencodeDb(env);
  try {
    await fs.access(file);
  } catch {
    return null;
  }
  let sqlite;
  try {
    sqlite = await import("node:sqlite");
  } catch {
    // A runtime without node:sqlite falls back rather than throwing.
    return null;
  }
  let db;
  try {
    db = new sqlite.DatabaseSync(file, { readOnly: true });
    return fn(db);
  } catch {
    return null;
  } finally {
    try {
      db?.close();
    } catch {
      // closing what we could not open is not an error worth having
    }
  }
}

async function readOpencodeDb(env, sessionId) {
  return await withOpencodeDb(env, (db) => {
    const messages = db
      .prepare("select id, data from message where session_id = ? order by time_created, id")
      .all(sessionId);
    if (messages.length === 0) return null;
    const parts = db
      .prepare("select message_id, data from part where session_id = ? order by time_created, id")
      .all(sessionId);

    const textByMessage = new Map();
    for (const row of parts) {
      let parsed;
      try {
        parsed = JSON.parse(row.data);
      } catch {
        continue;
      }
      if (parsed?.type !== "text" || !parsed.text) continue;
      const existing = textByMessage.get(row.message_id) ?? [];
      existing.push(String(parsed.text));
      textByMessage.set(row.message_id, existing);
    }

    const turns = [];
    for (const message of messages) {
      let parsed;
      try {
        parsed = JSON.parse(message.data);
      } catch {
        continue;
      }
      const role = parsed?.role;
      if (role !== "user" && role !== "assistant") continue;
      const text = readable((textByMessage.get(message.id) ?? []).join("\n").trim(), role);
      if (text.length > 0) turns.push({ role, text });
    }
    return turns;
  });
}

async function readOpencode(env, sessionId) {
  const fromDb = await readOpencodeDb(env, sessionId);
  if (fromDb !== null) return fromDb;
  return await readOpencodeFiles(env, sessionId);
}

/**
 * The pre-migration layout: `message/<session>/<msg>.json` holds the role,
 * `part/<msg>/*.json` holds what was said.
 */
async function readOpencodeFiles(env, sessionId) {
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
  const fromDb = await withOpencodeDb(env, (db) => {
    const row = db
      .prepare(
        "select id from session where directory = ? and time_created >= ? order by time_created desc limit 1",
      )
      .get(cwd, since);
    return row?.id ?? null;
  });
  if (fromDb !== null) return fromDb;
  return await discoverOpencodeSessionFiles(cwd, since, env);
}

/** The frozen JSON layout, for a machine that has not run opencode since January. */
async function discoverOpencodeSessionFiles(cwd, since, env) {
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

/**
 * The session a fresh interactive codex just created — found, not told.
 *
 * `codex "<prompt>"` opens its own window seeded with that prompt, but unlike
 * `exec --json` it announces no thread id: an interactive codex talks to a
 * person, not to us. Its rollout file knows, though — the first line is a
 * `session_meta` carrying the `cwd`, written when the file is created. So the
 * caller spawns the window, remembers the clock, and asks the store which
 * session appeared in that directory since.
 *
 * The id comes from the FILENAME rather than the payload, because that is what
 * `harnessTurns` matches on and what `codex resume` accepts — a forked session
 * carries a different `session_id` in its metadata than the file it lives in.
 *
 * Two rules here were paid for live (2026-08-28), and both are about not
 * handing back a session that belongs to somebody else:
 *
 * A candidate is judged on when its session was CREATED, never on when its
 * file was last written. The lead asking for this is often itself a codex in
 * the same directory: the moment it takes a turn, its rollout becomes the most
 * recently modified file in that cwd, and ranking by mtime would name the
 * lead's own conversation as the agent's.
 *
 * And when the caller knows what it seeded the window with, that text decides
 * — nothing else can. codex asks "trust this directory?" before it opens a
 * session at all, so the search may have to outlast a person answering it (32
 * minutes, in the run that taught us this), and anything else appearing in
 * that directory meanwhile is not ours. Without a seed the earliest session
 * created since is the best guess available, which is what a caller falls back
 * to once the window is gone and no better answer is coming.
 */
export async function discoverCodexSession(cwd, since, env = process.env, options = {}) {
  const { seed = null } = options;
  const root = codexRoot(env);
  const files = [];
  await collectFiles(root, (name) => name.startsWith("rollout-") && name.endsWith(".jsonl"), files);

  const candidates = [];
  for (const file of files) {
    let stat;
    try {
      stat = await fs.stat(file);
    } catch {
      continue;
    }
    // A file untouched since `since` cannot hold a session created after it —
    // the cheap test that keeps this off every rollout ever recorded.
    if (stat.mtimeMs < since) continue;
    const head = await readHead(file);
    let meta;
    try {
      meta = JSON.parse(head[0] ?? "");
    } catch {
      continue;
    }
    if (meta?.type !== "session_meta" || meta.payload?.cwd !== cwd) continue;
    const created = Date.parse(meta.payload?.timestamp ?? meta.timestamp ?? "");
    if (!Number.isFinite(created) || created < since) continue;
    // rollout-<timestamp>-<uuid>.jsonl — the uuid is the last 36 characters.
    candidates.push({ id: path.basename(file, ".jsonl").slice(-36), created, head });
  }
  candidates.sort((a, b) => a.created - b.created);
  const ours = seed === null ? candidates[0] : candidates.find((one) => carriesSeed(one.head, seed));
  return ours?.id ?? null;
}

/**
 * The opening of a session file, in lines.
 *
 * Bounded, because a live rollout runs to megabytes and this is read in a
 * poll: the metadata is the first line and the seeded prompt is among the
 * first few turns, so the opening is all this ever needs. A packet longer than
 * the window simply does not match, which costs a guess, never a wrong answer.
 */
async function readHead(file, bytes = 512 * 1024) {
  let handle;
  try {
    handle = await fs.open(file, "r");
    const buffer = Buffer.alloc(bytes);
    const { bytesRead } = await handle.read(buffer, 0, bytes, 0);
    const lines = buffer.subarray(0, bytesRead).toString("utf8").split("\n");
    // A full read stopped mid-file, so the last line is a fragment.
    if (bytesRead === bytes) lines.pop();
    return lines;
  } catch {
    return [];
  } finally {
    try {
      await handle?.close();
    } catch {
      // closing what we could not open is not an error worth having
    }
  }
}

/**
 * Is this the session we seeded? codex records the prompt it was launched with
 * as an ordinary user turn, so our own text is in the file verbatim. Compared
 * with whitespace collapsed, because the only difference a TUI is entitled to
 * make to text it echoes is how it wraps it.
 */
function carriesSeed(lines, seed) {
  const squash = (text) => String(text).replace(/\s+/g, " ").trim();
  const wanted = squash(seed);
  if (wanted.length === 0) return false;
  for (const line of lines) {
    let event;
    try {
      event = JSON.parse(line);
    } catch {
      continue;
    }
    const payload = event?.payload ?? {};
    if (payload.role !== "user" && payload.type !== "user_message") continue;
    const text =
      typeof payload.message === "string"
        ? payload.message
        : (payload.content ?? []).map((part) => part?.text ?? "").join("\n");
    if (squash(text).includes(wanted)) return true;
  }
  return false;
}

async function collectFiles(root, matches, into, depth = 6) {
  if (depth < 0) return;
  let entries;
  try {
    entries = await fs.readdir(root, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    const full = path.join(root, entry.name);
    if (entry.isFile() && matches(entry.name)) into.push(full);
    else if (entry.isDirectory()) await collectFiles(full, matches, into, depth - 1);
  }
}

/**
 * The session a kimi run created — found, not told.
 *
 * Kimi Code prints its id in a `session.resume_hint` at the END of the stream,
 * which means a run that dies before finishing takes the id with it. That is
 * not hypothetical: a 24-minute rebuild hit the provider's rate limit on its
 * last step and left a conversation nobody could resume, with all its work
 * intact on disk (live, 2026-08-24).
 *
 * Its store knows regardless. Each session's `state.json` records the `cwd` it
 * belongs to and when it was created, so the caller can ask afterwards which
 * session appeared in this directory — the same recovery opencode and codex
 * already have, and the reason none of the three depends on a stream surviving.
 */
export async function discoverKimiSession(cwd, since, env = process.env) {
  const root = kimiRoot(env);
  let workspaces;
  try {
    workspaces = await fs.readdir(root, { withFileTypes: true });
  } catch {
    return null;
  }

  let best = null;
  for (const workspace of workspaces) {
    if (!workspace.isDirectory()) continue;
    let sessions;
    try {
      sessions = await fs.readdir(path.join(root, workspace.name), { withFileTypes: true });
    } catch {
      continue;
    }
    for (const session of sessions) {
      if (!session.isDirectory()) continue;
      let state;
      try {
        state = JSON.parse(
          await fs.readFile(path.join(root, workspace.name, session.name, "state.json"), "utf8"),
        );
      } catch {
        continue;
      }
      const created = state?.createdAt ?? 0;
      if (state?.cwd !== cwd || created < since) continue;
      if (best === null || created > best.created) best = { id: state.id ?? session.name, created };
    }
  }
  return best?.id ?? null;
}
