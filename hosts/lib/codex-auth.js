// ChatGPT OAuth credentials for the gpt-image-2 backend, read from the Codex CLI's own auth
// store (the CC analog of pi's ctx.modelRegistry openai-codex token). Read-only: token refresh
// stays the codex CLI's job — an expired token surfaces as a 401 with a fix-it hint upstream.
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const JWT_AUTH_CLAIM = "https://api.openai.com/auth";

// Extract the chatgpt_account_id claim from a ChatGPT OAuth JWT. It lived in
// image.js beside the direct-API client that sent it as a header; that client
// is gone, and the only caller left is the login reader below.
export function decodeChatGptAccountId(token) {
  const parts = String(token ?? "").split(".");
  if (parts.length !== 3 || !parts[1]) {
    throw new Error("Codex token is not a JWT — run `codex login` (ChatGPT Plus/Pro).");
  }
  let payload;
  try {
    payload = JSON.parse(Buffer.from(parts[1], "base64url").toString("utf8"));
  } catch (error) {
    throw new Error(`Failed to decode Codex token: ${error instanceof Error ? error.message : String(error)}`);
  }
  const claims = payload?.[JWT_AUTH_CLAIM];
  const accountId = claims && typeof claims === "object" ? claims.chatgpt_account_id : undefined;
  if (typeof accountId !== "string" || !accountId) {
    throw new Error("Codex token has no chatgpt_account_id — run `codex login` again.");
  }
  return accountId;
}

export function codexAuthPath() {
  return path.join(process.env.CODEX_HOME || path.join(os.homedir(), ".codex"), "auth.json");
}

// Returns { token, accountId }. Throws with a `codex login` hint when anything is missing.
export async function loadCodexAuth() {
  const authPath = codexAuthPath();
  let raw;
  try {
    raw = await fs.readFile(authPath, "utf8");
  } catch {
    throw new Error(`No Codex CLI login found (${authPath}). Run \`codex login\` (ChatGPT Plus/Pro) to use image agents.`);
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error(`Could not parse ${authPath} — run \`codex login\` again.`);
  }
  const tokens = parsed?.tokens;
  const token = typeof tokens?.access_token === "string" && tokens.access_token ? tokens.access_token : undefined;
  if (!token) {
    throw new Error(`${authPath} has no ChatGPT access token — run \`codex login\` (an API key alone cannot drive the gpt-image-2 backend).`);
  }
  let accountId = typeof tokens.account_id === "string" && tokens.account_id ? tokens.account_id : undefined;
  if (!accountId) {
    // Older auth files may lack the explicit field; the JWT claim carries it.
    try {
      accountId = decodeChatGptAccountId(token);
    } catch (error) {
      if (typeof tokens.id_token !== "string" || !tokens.id_token) throw error;
      accountId = decodeChatGptAccountId(tokens.id_token);
    }
  }
  return { token, accountId };
}
