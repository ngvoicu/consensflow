# Agent catalog redesign — confirmed scope

2026-09-10. Gabriel requested a coherent redesign of Agents, including:

- Group Ready-made by harness, model and reasoning effort.
- Keep added presets visible with an Already added action state.
- Categories for coding, lead, PM and images, with useful descriptions.
- Use Artificial Analysis to inform recommendations.
- Add Astra and Fable low/medium variants across compatible harnesses.
- Investigate GPT Image 2.5 for Pygmalion.
- Apply the same grouping and categories to Your agents.

Confirmed answers:

1. Lead and PM are recommendations only. No catalog launch actions or role changes.
2. Your agents gets the same browsing controls and categorization as Ready-made.
3. Pygmalion must keep the existing Codex login; no separately billed Images API.
4. Pi must not be directed to Anthropic API usage as a substitute for a subscription.

5. Gabriel answered the Pi Fable choice: "keep the openrouter". Curated Pi Fable
   entries therefore use OpenRouter and identify its paid API route. Existing
   saved agents retain their provider until an explicit catalog update.

Earlier instructions remain relevant: reinstall after implementation, preserve
histories and profiles, and keep the simplified Harnesses and Agents screens.
Alpha.45 is the current local version. No new release publication is requested.

Proposed defaults for the spec: one shared toolbar controls both lists; default
grouping is Harness; categories are overlapping recommendation filters; custom
descriptions remain separate from curated recommendations. No new UI framework,
live benchmark service, arbitrary model upgrade, or provider migration.
