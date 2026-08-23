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
  } = input;

  const sections = [];
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
