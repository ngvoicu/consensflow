// Handoff size is bounded so a long session's transcript can't bloat a participant packet to where
// its model stops converging (a ~120 KB handoff made glm-5.2 at thinking=high stall instead of
// answering). capTail keeps the most recent tail, so a smaller budget preserves recent context and
// drops the oldest. Tunable via CONSENSFLOW_HANDOFF_MAX_BYTES without a reinstall.
const DEFAULT_MAX_BYTES = 48 * 1024;
const TOOL_RESULT_MAX_CHARS = 1500;
// Lead-initiated consultations (cf_run_participant) live only as tool results — there is no
// custom_message for them. Keep them generous so they cross-pollinate like @mention replies — but
// bounded to a fraction of the default budget so one consultation can't crowd out the rest of the
// recent conversation. Tunable via CONSENSFLOW_HANDOFF_CF_RESULT_MAX_CHARS.
const DEFAULT_CF_TOOL_RESULT_MAX_CHARS = 16000;
const TOOL_ARGS_MAX_CHARS = 200;

// Read a positive integer from an env var, falling back when unset/blank/non-numeric.
function envPositiveInt(name, fallback) {
  const raw = process.env[name];
  if (raw === undefined || raw === "") return fallback;
  const value = Number(raw);
  return Number.isFinite(value) && value > 0 ? Math.floor(value) : fallback;
}

// Serialize the active session branch (as returned by sessionManager.getBranch(), which is ordered
// root -> leaf / chronological) into readable text for a participant handoff. Honors the latest
// compaction (drops the messages it summarized), flattens AgentMessage content, and caps the total
// size keeping the most recent (tail) end.
export function serializeTranscript(branch, options = {}) {
  const maxBytes = options.maxBytes ?? envPositiveInt("CONSENSFLOW_HANDOFF_MAX_BYTES", DEFAULT_MAX_BYTES);
  const entries = Array.isArray(branch) ? branch : [];
  if (entries.length === 0) return "";

  // Respect the latest compaction: everything before firstKeptEntryId is replaced by its summary.
  let startIndex = 0;
  let preamble = null;
  for (let i = entries.length - 1; i >= 0; i -= 1) {
    if (entries[i].type === "compaction") {
      preamble = entries[i].summary;
      const keptIndex = entries.findIndex((entry) => entry.id === entries[i].firstKeptEntryId);
      if (keptIndex >= 0) startIndex = keptIndex;
      break;
    }
  }

  const blocks = [];
  if (preamble && String(preamble).trim()) blocks.push(`[Earlier conversation summary]\n${String(preamble).trim()}`);
  for (let i = startIndex; i < entries.length; i += 1) {
    const block = serializeEntry(entries[i]);
    if (block) blocks.push(block);
  }
  if (blocks.length === 0) return "";

  return capTail(blocks.join("\n\n"), maxBytes);
}

function serializeEntry(entry) {
  if (!entry || typeof entry !== "object") return null;
  if (entry.type === "branch_summary") return entry.summary ? `[Branch summary]\n${entry.summary}` : null;
  if (entry.type === "custom_message") return serializeCustomMessage(entry);
  if (entry.type !== "message") return null;
  return serializeMessage(entry.message);
}

// ConsensFlow participant replies are persisted as custom_message entries (via sendMessage), not
// normal messages — surface them so a later participant's handoff includes earlier @participant
// exchanges (cross-pollination). The triggering prompt rides along in details (the @mention input
// is "handled" by the extension and never recorded as a normal message).
export function serializeCustomMessage(entry) {
  if (!entry || typeof entry !== "object") return null;
  const details = entry.details;
  // Live participant stream crumbs are displayed in the main session for observability, but they
  // are not durable participant replies. Keep later handoffs focused on the final response.
  if (details?.streamEvent) return null;
  const participantId = details?.participant?.id;
  if (entry.customType === "consensflow" && participantId) {
    const lines = [];
    const prompt = details.prompt && String(details.prompt).trim();
    if (prompt) lines.push(`User → @${participantId}: ${prompt}`);
    const reply = String(details.output ?? flattenContent(entry.content) ?? "").trim();
    if (reply) lines.push(`@${participantId} replied:\n${reply}`);
    return lines.length ? lines.join("\n") : null;
  }
  const text = flattenContent(entry.content);
  return text ? `Note:\n${text}` : null;
}

export function serializeMessage(message) {
  if (!message || typeof message !== "object") return null;
  switch (message.role) {
    case "user": {
      const text = flattenContent(message.content);
      return text ? `User:\n${text}` : null;
    }
    case "assistant": {
      const text = flattenContent(message.content);
      return text ? `Lead:\n${text}` : null;
    }
    case "toolResult": {
      const maxChars = message.toolName === "cf_run_participant"
        ? envPositiveInt("CONSENSFLOW_HANDOFF_CF_RESULT_MAX_CHARS", DEFAULT_CF_TOOL_RESULT_MAX_CHARS)
        : TOOL_RESULT_MAX_CHARS;
      const body = truncate(flattenContent(message.content), maxChars);
      const label = message.toolName ? ` ${message.toolName}` : "";
      return body ? `Tool result${label}${message.isError ? " (error)" : ""}:\n${body}` : null;
    }
    case "custom": {
      const text = flattenContent(message.content);
      return text ? `Note:\n${text}` : null;
    }
    case "compactionSummary":
      return message.summary ? `[Earlier conversation summary]\n${message.summary}` : null;
    case "branchSummary":
      return message.summary ? `[Branch summary]\n${message.summary}` : null;
    case "bashExecution":
      return message.command ? `$ ${message.command}\n${truncate(message.output ?? "", TOOL_RESULT_MAX_CHARS)}` : null;
    default:
      return null;
  }
}

function flattenContent(content) {
  if (typeof content === "string") return content.trim();
  if (!Array.isArray(content)) return "";
  const parts = [];
  for (const part of content) {
    if (typeof part === "string") {
      parts.push(part);
      continue;
    }
    if (!part || typeof part !== "object") continue;
    switch (part.type) {
      case "text":
        if (part.text) parts.push(part.text);
        break;
      case "toolCall":
        parts.push(`→ ${part.name}(${truncate(safeJson(part.arguments), TOOL_ARGS_MAX_CHARS)})`);
        break;
      case "image":
        parts.push("[image]");
        break;
      case "thinking":
        break; // omit reasoning from the handoff
      default:
        if (typeof part.text === "string") parts.push(part.text);
    }
  }
  return parts.filter(Boolean).join("\n").trim();
}

function safeJson(value) {
  try {
    return JSON.stringify(value ?? {});
  } catch {
    return "{}";
  }
}

function truncate(value, maxChars) {
  const text = String(value ?? "").trim();
  if (text.length <= maxChars) return text;
  return `${text.slice(0, maxChars)}…[truncated]`;
}

// Keep the most recent tail (handoff context favors recent state), prefixing a marker when cut.
function capTail(text, maxBytes) {
  if (Buffer.byteLength(text, "utf8") <= maxBytes) return text;
  const marker = "[earlier handoff truncated]\n\n";
  const budget = Math.max(0, maxBytes - Buffer.byteLength(marker, "utf8"));
  let tail = text.slice(-budget);
  while (Buffer.byteLength(tail, "utf8") > budget) tail = tail.slice(1);
  return `${marker}${tail}`;
}
