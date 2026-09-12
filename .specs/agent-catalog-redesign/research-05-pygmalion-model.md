# Pygmalion image model label

Reviewed 2026-09-10 using OpenAI Docs and the existing runner. Documentation research only; no image generation, model calls, credentials, or user-state access.

**Keep “Codex Images” with “Codex login.” There is no verified basis to label the existing Pygmalion route “GPT Image 2.5,” “Sunburst,” or “Flare.”**

The current official OpenAI image-generation documentation explicitly identifies the built-in image model as `gpt-image-2`. It describes usage as counting against general Codex limits, and separately describes API-backed generation. This is documentation of the built-in route, not a guarantee that ConsensFlow can pin its model or observe the backing model of every run. [Official OpenAI image-generation documentation](https://learn.chatgpt.com/docs/image-generation)

GPT Image 2.5 is an API model family with two explicit selectors: `gpt-image-2.5-sunburst` and `gpt-image-2.5-flare`. OpenAI's model pages say each can be selected directly in the Image API or as the model of the Responses API image-generation tool. Sunburst emphasizes precise editing; Flare emphasizes fast generation. These pages do not establish that Codex login automatically uses either model. [Sunburst model documentation](https://developers.openai.com/api/docs/models/gpt-image-2.5-sunburst), [Flare model documentation](https://developers.openai.com/api/docs/models/gpt-image-2.5-flare)

In `hosts/lib/image-run.js`, `runImageAgent` instructs Codex to invoke its image-generation tool and `runCodex` launches `codex exec` without an image-model parameter. The child environment excludes `OPENAI_API_KEY`, preserving the intended Codex-login route. The result records `backend: "codex-image"`; it does not record a verified image-model identity. Editing the preset label or passing a model-like name in natural language would not establish model selection.

Recommended user-facing explanation: **“Pygmalion uses image generation through your Codex login. OpenAI currently documents that built-in route as GPT Image 2; ConsensFlow does not select the underlying image model.”** Keep the product card's concise “Codex Images” label, preserve the existing login route, and do not attach GPT Image 2.5 benchmark scores. A future selectable Sunburst/Flare route would require separate implementation and the user's authorization for an API route; it is outside the current request and conflicts with the user's previous choice to keep Codex login.
