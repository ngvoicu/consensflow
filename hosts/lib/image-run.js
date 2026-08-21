import fs from "node:fs/promises";
import path from "node:path";
import { loadCodexAuth } from "./codex-auth.js";
import { generateImage, IMAGE_BACKEND, IMAGE_TRIGGER_DEFAULT, imageFileToDataUrl, saveImagePng } from "./image.js";
import { recordLatestRun, runsRoot } from "./state.js";
import { createId } from "./utils.js";

/**
 * An image agent's run, shared by every caller.
 *
 * It is a spawn like any other — same run directory, same result.json, same
 * "latest run" record — but there is no harness to launch: gpt-image-2 is
 * reached through the Codex CLI's own login, so the packet has nothing to
 * carry and the prompt goes straight to the backend.
 */
export async function runImageAgent(input) {
  const { cwd, agent, prompt, imagePaths = [] } = input;
  const { token, accountId } = await loadCodexAuth();
  const runId = createId("image");
  const runDir = path.join(runsRoot(cwd), runId);
  await fs.mkdir(runDir, { recursive: true });

  const triggerModel = agent.model || IMAGE_TRIGGER_DEFAULT;
  // Reference images (`--image <path>`, repeatable) become input_image parts so
  // gpt-image-2 can edit or condition on them.
  const images = await Promise.all(imagePaths.map((p) => imageFileToDataUrl(p)));
  const image = await generateImage({ token, accountId, prompt, triggerModel, images });
  const savedPath = await saveImagePng(image.base64, runDir, "image.png");

  const result = {
    schemaVersion: 1,
    runId,
    runDir,
    savedPath,
    kind: "image",
    backend: IMAGE_BACKEND,
    triggerModel,
    referenceImages: imagePaths,
    revisedPrompt: image.revisedPrompt,
    responseId: image.responseId,
    agent: { id: agent.id, kind: agent.kind },
  };
  await fs.writeFile(path.join(runDir, "result.json"), `${JSON.stringify(result, null, 2)}\n`, "utf8");
  await recordLatestRun(cwd, { runId, runDir, agent, kind: "image" });
  return result;
}

/** The same answer text every caller prints for an image run. */
export function renderImageRun(result) {
  return [
    `# @${result.agent.id}`,
    "",
    `Generated an image with **${result.backend}** (via your Codex CLI login).`,
    result.referenceImages?.length ? `Reference image(s): ${result.referenceImages.join(", ")}` : undefined,
    result.revisedPrompt ? `Revised prompt: ${result.revisedPrompt}` : undefined,
    `Saved: ${result.savedPath}`,
    "",
    "View it with the Read tool if needed.",
  ]
    .filter(Boolean)
    .join("\n");
}
