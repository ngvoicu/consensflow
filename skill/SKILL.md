---
name: consensflow
description: Consult one of the user's named AI participants — zeus, calliope, hyperion, gaia, diana, endymion, mani, aether, rhea, phoebe, sunna, jord, bil, prometheus, hephaestus — each a real coding-agent CLI (claude, codex, pi, opencode) run one-shot in the current directory. Use whenever the user says "ask <name> …", "what does <name> think", "consult <name>", "get a second opinion from <name>", or names any participant — and also when you yourself want an independent second opinion on a risky or debatable decision.
---

# ConsensFlow participants

The user keeps a roster of named AI participants. Each participant is a real,
separately-installed coding-agent CLI with a fixed model and effort. Consulting
one means running its one-shot command below with your question as the final
argument. The participant runs in **your current working directory** — it can
read the project's files itself, so you do not need to paste file contents.

## How to consult

1. Pick the participant the user named (or choose one yourself: prefer
   `calliope`/`zeus` for deep review, `diana`/`bil` for quick checks).
2. Compose the question: one or two sentences of task context, then the
   concrete question. Name specific files with relative paths when relevant.
3. Run the participant's exact command from the table below, replacing only
   `<question>`. Run it from the project directory. Turns can take minutes at
   high effort — use a generous timeout (10+ minutes for max/ultra).
4. Report the answer to the user **verbatim or faithfully summarized, and
   attributed** ("hyperion says: …"). Never present a participant's answer as
   your own.

## Rules

- **One participant at a time.** Wait for one answer before asking another.
- **Advice is free; acting is gated.** Never apply a participant's suggested
  changes, or keep files it created, without the user's explicit approval —
  unless the user already authorized it in this conversation.
- **Do not retry a slow participant with a different one** unless the command
  itself failed. Slow usually means thinking.

## Roster

| Participant | Runs | Command (replace `<question>` only) |
|---|---|---|
| **zeus** — Claude Opus 5, max effort; high-stakes architecture and review | claude | `env -u ANTHROPIC_API_KEY claude -p "<question>" --model claude-opus-5 --effort max` |
| **calliope** — Claude Fable 5.1, max effort; the deepest reviewer on the roster | claude | `env -u ANTHROPIC_API_KEY claude -p "<question>" --model claude-fable-5-1 --effort max` |
| **hyperion** — GPT 5.6 Sol, ultra effort; deepest Codex participant | codex | `env -u OPENAI_API_KEY codex exec --skip-git-repo-check -m gpt-5.6-sol -c model_reasoning_effort="ultra" "<question>"` |
| **gaia** — GPT 5.6 Terra, xhigh; balanced Codex | codex | `env -u OPENAI_API_KEY codex exec --skip-git-repo-check -m gpt-5.6-terra -c model_reasoning_effort="xhigh" "<question>"` |
| **diana** — GPT 5.6 Luna, xhigh; fast Codex checks | codex | `env -u OPENAI_API_KEY codex exec --skip-git-repo-check -m gpt-5.6-luna -c model_reasoning_effort="xhigh" "<question>"` |
| **endymion** — Kimi K3 (1M context), xhigh; huge-context reads | pi | `pi --no-session --model openrouter/moonshotai/kimi-k3 --thinking xhigh -p "<question>"` |
| **aether** — GPT 5.6 Sol via pi, xhigh | pi | `pi --no-session --model openai-codex/gpt-5.6-sol --thinking xhigh -p "<question>"` |
| **rhea** — GPT 5.6 Terra via pi, xhigh | pi | `pi --no-session --model openai-codex/gpt-5.6-terra --thinking xhigh -p "<question>"` |
| **phoebe** — GPT 5.6 Luna via pi, xhigh | pi | `pi --no-session --model openai-codex/gpt-5.6-luna --thinking xhigh -p "<question>"` |
| **prometheus** — GLM 5.2, high | pi | `pi --no-session --model openrouter/z-ai/glm-5.2 --thinking high -p "<question>"` |
| **hephaestus** — Qwen3.7 Max, high | pi | `pi --no-session --model openrouter/qwen/qwen3.7-max --thinking high -p "<question>"` |
| **mani** — Kimi K3 via opencode | opencode | `opencode run --model openrouter/moonshotai/kimi-k3 "<question>"` |
| **sunna** — GPT 5.6 Sol via opencode, xhigh | opencode | `opencode run --model openrouter/openai/gpt-5.6-sol --variant xhigh "<question>"` |
| **jord** — GPT 5.6 Terra via opencode, xhigh | opencode | `opencode run --model openrouter/openai/gpt-5.6-terra --variant xhigh "<question>"` |
| **bil** — GPT 5.6 Luna via opencode, xhigh | opencode | `opencode run --model openrouter/openai/gpt-5.6-luna --variant xhigh "<question>"` |

The `env -u …_API_KEY` prefixes are deliberate: they keep subscription logins
from silently switching to API-key billing. Keep them.

(`pygmalion`, the image participant, is not yet supported by this skill.)

## Visible pane (optional)

By default, run the command inline and read its output. If the user asks to
*watch* the participant work and the cmux skills are installed, open a split in
the current cmux workspace with those skills and run the same command there —
this skill defines *what* to run; the cmux skills define pane control.

## Roster maintenance

The roster above is generated. To change it, the user runs `cf participant …`
or the ConsensFlow UI — never edit this file by hand; it will be regenerated.
