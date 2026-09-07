import { nowIso } from "./utils.js";

export async function createPacket(input) {
  const {
    cwd,
    agent,
    task,
    brief = "",
    extraContext = "",
    handoff = "",
    continuing = false,
    conversational = false,
    nonce = null,
  } = input;

  const sections = [];
  // A launch nonce is non-secret launch evidence, not content: it rides on
  // the FIRST line so the harness stores it verbatim in the session's first
  // user turn, and the display reader strips it before anyone sees it. Same
  // argument and same placement as `createWindowSeed` below, because a
  // window's seed and a packet are the same thing to the harness that
  // receives one: kimi opens no window and takes its prompt in argv, so a
  // packet is the only text its store ever sees. One rule, in one place.
  if (nonce !== null && nonce !== undefined && String(nonce).trim() !== "") {
    sections.push(formatLaunchMarker(nonce));
  }
  // A follow-up in a live conversation needs none of the scene-setting: the
  // agent is already in this workspace and has already been told how to work.
  // Re-sending it every turn buries the actual question — and in an attached
  // window the user scrolls past three screens of boilerplate to reach one
  // line of answer.
  if (!continuing) {
    sections.push("# ConsensFlow Packet");
    sections.push(`Created: ${nowIso()}`);
    sections.push(`Workspace: ${cwd}`);
    sections.push("");
  }

  // No "who you are": the work decides what this run is, not a persona. A
  // brief is the lead's own words about what it wants from THIS spawn — a
  // GDPR review, a second opinion on a migration — so it leads the packet
  // when there is one, and nothing stands in for it when there is not.
  if (brief && String(brief).trim()) {
    sections.push("## Your brief for this run");
    sections.push(String(brief).trim());
    sections.push("");
  }

  if (!continuing) {
    sections.push("## How to work");
    sections.push("You can read and modify this workspace — edit files and run commands as needed.");
    sections.push("When the task involves analyzing or changing anything here, work iteratively: read, grep, and run commands or tests to verify claims as you go, rather than reasoning out a long answer in one pass. Explore first, then answer.");
    sections.push("");
  }

  if (handoff && String(handoff).trim()) {
    sections.push("## Handoff — current session");
    sections.push("The conversation so far between the user and the lead, most recent last. You were not part of it; use it as context for the request below.");
    sections.push("");
    sections.push(String(handoff).trim());
    sections.push("");
  }

  if (extraContext && String(extraContext).trim()) {
    sections.push("## Note from the lead");
    sections.push(String(extraContext).trim());
    sections.push("");
  }

  sections.push("## Message from the user");
  sections.push(taskForKind("ask", task));
  sections.push("");
  sections.push("Respond directly and conversationally. There is no required format.");
  // A one-shot agent that asks a question is talking to nobody: the next run is
  // a stranger who never heard it, so guessing was the only sensible move. In a
  // conversation the reply comes back to THIS session with your memory intact,
  // which makes asking the right move rather than a dead end.
  if (conversational) {
    sections.push("");
    sections.push(
      "This is a conversation, not a one-shot: if the task is ambiguous, say what you need instead of guessing — the answer will reach you in the same conversation, with everything above still in mind.",
    );
  }
  sections.push("");
  return sections.join("\n");
}

export function taskForKind(_kind, baseTask) {
  return String(baseTask ?? "").trim() || "Respond to the user's message.";
}

/**
 * The first message of a window — not a packet.
 *
 * The packet's scene-setting exists for one-shot runs: the header timestamps
 * an artifact, and "How to work" keeps a one-shot model from reasoning out a
 * long answer instead of exploring. A window needs neither — it is a full
 * interactive session that already knows how to work, and its seed is the
 * first thing the USER sees in their pane. Three screens of scaffolding above
 * one line of question is exactly what `continuing` was invented to stop.
 *
 * So a bare task travels bare. When a brief, note or handoff rides along, the
 * "## Message from the user" marker stays — `cf catchup` unwraps on it, and
 * without it the scaffolding would read back as "you asked".
 */
export function createWindowSeed(input) {
  const { task, brief = "", extraContext = "", handoff = "", nonce = null } = input;
  const sections = [];
  // A launch nonce is non-secret launch evidence, not content: it rides on
  // the seed's FIRST line so the harness stores it verbatim in the session's
  // first user turn, and the display reader strips it before anyone sees it.
  if (nonce !== null && nonce !== undefined && String(nonce).trim() !== "") {
    sections.push(formatLaunchMarker(nonce));
  }
  if (brief && String(brief).trim()) {
    sections.push("## Your brief for this run", String(brief).trim(), "");
  }
  if (handoff && String(handoff).trim()) {
    sections.push(
      "## Handoff — current session",
      "The conversation so far between the user and the lead, most recent last. You were not part of it; use it as context for the request below.",
      "",
      String(handoff).trim(),
      "",
    );
  }
  if (extraContext && String(extraContext).trim()) {
    sections.push("## Note from the lead", String(extraContext).trim(), "");
  }
  const message = String(task ?? "").trim() || "Respond to the user's message.";
  if (sections.length === 0) return message;
  sections.push("## Message from the user", message);
  return sections.join("\n");
}

/**
 * Launch-unique evidence for native-session binding.
 *
 * `[consensflow launch <nonce>]` is deliberately non-secret: it is stored in
 * the harness's own session file and its only job is proving THIS launch
 * opened THAT session. Task text alone can never do that — two identical
 * prompts in one directory must bind to their own launches.
 */
export function formatLaunchMarker(nonce) {
  const clean = String(nonce ?? "").trim();
  if (clean.length === 0 || /[\[\]\r\n]/.test(clean)) {
    throw new Error("launch nonce must be a single line without brackets");
  }
  return `[consensflow launch ${clean}]`;
}

/** The nonce carried by one line, or null when the line carries none. */
export function parseLaunchNonce(line) {
  const match = /^\[consensflow launch ([^\]]+)\]\s*$/.exec(String(line ?? "").trim());
  const nonce = match?.[1]?.trim() ?? "";
  return nonce.length === 0 ? null : nonce;
}

/** Drop every launch-marker line; what a person reads never shows one. */
export function stripLaunchMarker(text) {
  return String(text ?? "")
    .split("\n")
    .filter((line) => parseLaunchNonce(line) === null)
    .join("\n")
    .trim();
}

/**
 * Injected blocks off the front, whatever a person wrote left standing.
 *
 * A COMPLETE `<tag>…</tag>` is what an environment injects — verified against
 * a real codex rollout on 2026-08-27, closing tag and all. A lone opening tag
 * is somebody talking: `<div> tags are escaping wrong` is a question, and
 * `<!doctype html>` is an answer — and so is a tag carrying attributes, which
 * the injector never writes. Repeated, because one turn can carry more than
 * one block. If a harness ever injects a block it does not close, this shows
 * it rather than hiding it — the harmless direction for a reader whose job is
 * to lose nothing.
 *
 * Lives here (not in harness-transcript.js) so the pure session-binding
 * module can use it without an import cycle.
 */
const INJECTED_BLOCK = /^\s*<([a-z][a-z0-9_-]*)>[\s\S]*?<\/\1>\s*/i;

export function withoutInjectedBlocks(text) {
  let rest = String(text ?? "");
  while (INJECTED_BLOCK.test(rest)) rest = rest.replace(INJECTED_BLOCK, "");
  return rest.trim();
}
