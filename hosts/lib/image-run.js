import fs from "node:fs/promises";
import path from "node:path";
import { loadCodexAuth } from "./codex-auth.js";
import { IMAGE_BACKEND } from "./image.js";
import { childEnv } from "./runners.js";
import { recordLatestRun, runsRoot } from "./state.js";
import { createId } from "./utils.js";
import { spawn } from "node:child_process";

/**
 * An image agent's run: ask codex to draw, rather than impersonate it.
 *
 * This used to call the Codex responses endpoint directly with our own
 * `image_generation` tool definition, presenting codex's token and its
 * originator. That path is closed (probed 2026-08-24):
 *
 *     400 The 'gpt-image-2' model is not supported when using Codex with a
 *         ChatGPT account.
 *
 * The restriction is real and documented upstream — a ChatGPT login is never
 * handed the image tool, and OpenAI's answer is "bring an API key". But codex
 * ITSELF still draws on that same login, because its own sessions are
 * provisioned with the tool we were not. So the fix is to stop imitating codex
 * and simply ask it: one `codex exec`, an instruction to use its image tool,
 * and a path to save to. Verified end to end on a ChatGPT Plus account, with
 * gpt-image-2 and no API key.
 *
 * What this costs is worth naming: reference images are described by path
 * rather than uploaded as parts, and the revised prompt is no longer visible.
 * What it buys is a product with one fewer API client in it — an image agent
 * is now a harness we hand a task to, like every other agent here.
 */
const IMAGE_TIMEOUT_MS = 10 * 60_000;

export async function runImageAgent(input) {
  const { cwd, agent, prompt, imagePaths = [], signal } = input;
  // Fail on a missing login before spawning anything. codex would refuse too,
  // but it would take a process and a stack trace to say so, and "no Codex CLI
  // login found" is the sentence that tells a user what to do.
  await loadCodexAuth();
  const runId = createId("image");
  const runDir = path.join(runsRoot(cwd), runId);
  await fs.mkdir(runDir, { recursive: true });
  const savedPath = path.join(runDir, "image.png");

  const references = imagePaths.filter((p) => typeof p === "string" && p.length > 0);
  const instruction = [
    "Use your image generation tool to generate exactly one image.",
    references.length > 0
      ? `Use these files as visual reference, reading them first: ${references.map((p) => path.resolve(cwd, p)).join(", ")}.`
      : undefined,
    "",
    "What to generate:",
    String(prompt ?? "").trim(),
    "",
    `Save the result to this exact absolute path: ${savedPath}`,
    "Do not write any other file. When it is saved, reply with one short line and nothing else.",
  ]
    .filter((line) => line !== undefined)
    .join("\n");

  const started = Date.now();
  const attempt = await runCodex(cwd, instruction, signal);
  // The file on disk is the only proof that matters: codex can answer
  // cheerfully and still not have drawn anything.
  const drawn = await fs
    .stat(savedPath)
    .then((s) => s.isFile() && s.size > 0)
    .catch(() => false);

  const result = {
    schemaVersion: 1,
    runId,
    runDir,
    savedPath: drawn ? savedPath : null,
    kind: "image",
    backend: IMAGE_BACKEND,
    via: "codex exec",
    referenceImages: references,
    ok: drawn,
    // A run that failed still leaves a record. Without one, nothing downstream
    // could tell a failure from a slow success: an empty run directory looked
    // exactly like work in progress, and a lead reported "it is running" for a
    // generation that had already died (live, 2026-08-24).
    error: drawn
      ? undefined
      : (firstError(attempt.output) ?? `codex exited ${attempt.code} without writing an image`),
    exitCode: attempt.code,
    durationMs: Date.now() - started,
    agent: { id: agent.id, kind: agent.kind },
  };
  await fs.writeFile(path.join(runDir, "result.json"), `${JSON.stringify(result, null, 2)}\n`, "utf8");
  await fs.writeFile(path.join(runDir, "codex.log"), attempt.output, "utf8");
  await recordLatestRun(cwd, { runId, runDir, agent, kind: "image" });
  return result;
}

function runCodex(cwd, instruction, signal) {
  return new Promise((resolve) => {
    const child = spawn(
      "codex",
      [
        "exec",
        "-C",
        cwd,
        "--dangerously-bypass-approvals-and-sandbox",
        "--skip-git-repo-check",
        instruction,
      ],
      {
        cwd,
        // The same guards every agent process gets: a stray OPENAI_API_KEY
        // here would quietly move an image onto per-token billing, which is
        // the one thing this path exists to avoid.
        env: childEnv(process.env, { env: { CONSENSFLOW_CHILD: "1" }, dropEnv: ["OPENAI_API_KEY"] }),
        stdio: ["ignore", "pipe", "pipe"],
        signal,
      },
    );
    let output = "";
    child.stdout.on("data", (chunk) => {
      output += chunk;
    });
    child.stderr.on("data", (chunk) => {
      output += chunk;
    });
    const timer = setTimeout(() => child.kill("SIGKILL"), IMAGE_TIMEOUT_MS);
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code: code ?? 0, output });
    });
    child.on("error", (cause) => {
      clearTimeout(timer);
      resolve({ code: 127, output: `${output}\n${cause}` });
    });
  });
}

/** The line worth showing a user out of a whole codex transcript. */
function firstError(output) {
  const match = /(?:error|Error|failed|not supported)[^\n]{0,200}/.exec(String(output ?? ""));
  return match?.[0]?.trim();
}

export function renderImageRun(result) {
  if (!result.ok) {
    return [
      `# @${result.agent.id}`,
      "",
      "No image was generated.",
      result.error ? `\n${result.error}` : undefined,
      `\nWhat codex said is in ${path.join(result.runDir, "codex.log")}.`,
    ]
      .filter(Boolean)
      .join("\n");
  }
  return [
    `# @${result.agent.id}`,
    "",
    `Generated an image with **${result.backend}**, drawn by codex on your ChatGPT login.`,
    result.referenceImages?.length ? `Reference image(s): ${result.referenceImages.join(", ")}` : undefined,
    `Saved: ${result.savedPath}`,
    "",
    "View it with the Read tool if needed.",
  ]
    .filter(Boolean)
    .join("\n");
}
