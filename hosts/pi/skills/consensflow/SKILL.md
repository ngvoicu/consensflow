---
name: consensflow
description: Use ConsensFlow inside pi to consult one of the user's named AI agents — each a chosen model at a chosen effort, run one-shot by its own harness (claude, codex, pi, opencode) — for second opinions, design and code critique, questions, implementation help, or real work in the project. Activate whenever the user names an agent (e.g. "@zeus what do you think") AND whenever you would yourself benefit from an independent opinion on a risky or debatable decision — consulting is free, encouraged, and needs no permission. Acting on what an agent says, or keeping files it changed, is gated behind explicit user approval.
---

# ConsensFlow

ConsensFlow lets the lead (this pi session) consult one named agent at a time. An agent is a model at a fixed effort, run one-shot by its own harness (claude / codex / opencode / pi) as an isolated subprocess: it receives a handoff of the current session plus your prompt, answers once, and does not persist between calls. It runs with full permissions — exactly like running that harness yourself with every approval already granted: it can read, edit files, run commands and reach the network, and it is not sandboxed to the project directory. Consulting an agent is like phoning an advisor who can also pick up a task. The lead stays the decision-maker, and ConsensFlow never accepts or keeps an agent's work on its own — inspect `git status` / `git diff` after any run before keeping changes.

## Reach for an advisor on your own

You do not need permission to consult, and you should not wait to be asked. A second opinion is cheap; a wrong decision the user has to unwind is not. Consult when:

- **you are about to finalize a non-trivial design or diff** — "review this approach; what is the riskiest assumption?";
- **a plan touches migrations, rollback, auth, money or data loss** — "poke holes in this: what breaks under load or on rollback?";
- **you are unsure between two designs** — ask one agent for a straight preference and a reason;
- **you have been stuck on the same failure twice** — a fresh reader with the same files often sees it immediately;
- **the user is about to act on your recommendation** and you have not had it checked by anyone.

Say who you asked and what they said. Do not consult in a loop: one agent, one question, then decide or ask the user.

## What agents can do

Use agents for all of these, one agent at a time. No preset is intrinsically review-only; the same agent can advise or do workspace work — by default it can read, edit files, and run commands in the workspace:

- **Advice / second opinion / design critique.** Ask an agent to inspect context, critique a plan, assess a pasted diff, identify risks, or suggest tests.
- **Doing work / code-writing help.** The same agent can implement, refactor, or run commands by default — full permissions, like a normal CLI run with every prompt pre-approved. Treat it like a temporary helper: after the run, inspect `git status` / `git diff` and relevant tests, then ask the user before keeping or building on the changes unless they pre-authorized it.
- **Image generation.** `@pygmalion` (or any `kind=image` agent) uses **gpt-image-2** via Pi's `openai-codex` login. It receives the image prompt only — no session handoff — saves `image.png` in the ConsensFlow run dir under `~/.consensflow/workspaces/…`, and Pi shows the generated image inline. Optionally pass one or more **reference images** with `--image <path>` (repeatable, or the `cf_run_agent` tool's `images` param) so gpt-image-2 edits/conditions on them — supply a file path (.png/.jpg/.jpeg/.webp/.gif).

## The two rules that matter most

These two rules are the heart of the skill. They pull in opposite directions on purpose: consulting is cheap and encouraged; acting on the result is gated.

### 1. Consulting is free — and encouraged

The lead may, and should, reach for an agent on its own initiative, with NO user permission needed merely to consult. Treat an agent consultation as a natural reflex, not a rare ceremony — before finalizing a non-trivial design or diff, consider getting another take. Just call `cf_run_agent`; you do not ask the user "may I consult X?" first. Good moments, with the kind of prompt to send:

- **Before finalizing a non-trivial design or diff** — `@zeus` "Review this approach; call out the riskiest assumption and anything you'd do differently."
- **To stress-test a plan** — `@hyperion` "Poke holes in this migration plan — what breaks under load or on rollback?"
- **To surface unknowns before building** — `@gaia` "What questions should I answer before I start implementing this?"
- **For a quick gut-check or tie-breaker** — `@nike` "Does this error handling read as correct to you — yes/no with one reason?"
- **For a focused diff/task check** — run `git diff` yourself and paste the relevant parts into the prompt or `context` brief.

### 2. Acting on the output is GATED — never without asking

The lead MUST NOT apply, merge, commit, adopt, integrate, or otherwise act on an agent's response — and MUST NOT keep or extend any files an agent edited — without first surfacing it to the user and getting explicit approval. This is a hard rule, not a preference.

Before acting, the lead MUST present:

- a concise **summary** of what the agent said or did, and
- the **lead's own recommendation** (accept / accept-with-changes / reject, and why).

Then wait for the user to approve.

This gate covers BOTH cases equally:

- **(a) Advice in a text response.** Do not implement, refactor toward, or commit to an agent's suggestion until the user approves it.
- **(b) Real changes an agent made.** An agent may have edited files or run commands in the workspace — it runs with full permissions by default. Do not treat that work as accepted: surface what changed (summary + recommendation) and get approval before keeping, building on, or committing it. If the user rejects it, revert it.

**The only exception:** the user has already explicitly told the lead to proceed — e.g. "get Zeus's take and apply what makes sense," or "run the builder and commit it." Pre-authorization scoped to that request stands in for the approval; do not re-ask. Absent such an instruction, never act on an agent's output on your own.

Do / Never, in one line each:

- **Do** consult an agent whenever a second opinion would help — no permission needed.
- **Never** apply, commit, or keep an agent's advice or file changes without the user's go-ahead, unless the user pre-authorized it.

In short: ask freely, apply only with a green light.

## How agents are created

Agents are configured in the shared roster `~/.consensflow/agents.json` (set up once, use from any project, Pi, and the Claude Code sibling). There are no per-tool config roots. Agents come from curated presets or fully custom definitions:

**The names below are a menu, not your roster.** None of them exists until it is added. Your actual agents are the ones `agents list` prints — and the line at the top of this session already named them. `@zeus` and friends appear throughout this skill only as placeholders in examples; substitute a name you actually have, and if the user asks for one that is not on the roster, say so and offer to add it rather than guessing a substitute.

```text
/consensflow:presets                            # list built-in presets
/consensflow:agents add zeus              # add a preset            → @zeus
/consensflow:agents add endymion          # Pi-backed Kimi K3 → @endymion
/consensflow:agents add all               # add every preset
/consensflow:agents add zeus --name Deepreview    # preset backend, renamed → @deepreview
/consensflow:agents add --name Builder --kind codex --model gpt-5.6-sol --effort high
                                                # fully custom; full permissions like every agent
```

Presets run with full permissions; the same model+effort family exists on every engine that runs it:

- **Fable 5** (Anthropic's top model — use for the questions that really matter): `@calliope`/`@clio`/`@euterpe`/`@thalia` (Claude Code max/xhigh/high/medium), `@orpheus`/`@linus`/`@erato` (Pi xhigh/high/medium, Anthropic auth), `@saga`/`@gunnlod`/`@kvasir` (OpenCode xhigh/high/medium via OpenRouter).
- **Sonnet 5**: `@hermod` (Claude Code, max effort).
- **Opus 5**: `@zeus`/`@apollo`/`@artemis` (Claude Code max/xhigh/medium), `@kronos`/`@atlas` (Pi xhigh/medium, Anthropic auth), `@baldr`/`@vali` (OpenCode xhigh/medium via OpenRouter).
- **GPT 5.6** (three variants: Sol flagship, Terra balanced, Luna fast): `@hyperion`/`@phoebus` (Codex Sol ultra/xhigh), `@gaia` (Codex Terra xhigh), `@diana` (Codex Luna xhigh), `@aether`/`@rhea`/`@phoebe` (Pi Sol/Terra/Luna xhigh, same ChatGPT login), `@sunna`/`@jord`/`@bil` (OpenCode Sol/Terra/Luna xhigh via OpenRouter).
- **Deep open-weights**: Kimi K3 — `@endymion` (Pi, xhigh thinking), `@mani` (OpenCode). K2.7 Code was retired in 1.9.0.
- **Fast/cheap tier** (quick gut-checks): `@nike`/`@sif` (Gemini 3.7 Flash on Pi/OpenCode), `@zephyros`/`@freya` (DeepSeek V4 Flash 0731 on Pi/OpenCode).
- **Model zoo** (same OpenRouter models on two engines; Greek = pi, Norse = opencode): DeepSeek V4 Pro 0813 `@hades`/`@odin`, Gemini 3.1 Pro `@helios`/`@heimdall`, Grok 4.6 `@ares`/`@thor`, Qwen3.8 Max `@hephaestus`/`@tyr`, MiniMax M3 `@metis`/`@mimir`, GLM 5.3 `@prometheus` (pi only).
- **Image**: `@pygmalion` (kind=image) generates a picture with gpt-image-2 via your existing openai-codex login — prompt-only (no handoff), optional `--image <path>` reference(s), saved to the workspace's run dir under `~/.consensflow/workspaces/…` and shown inline.

Run `/consensflow:presets` for the full list. Model and effort strings pass through to the engine verbatim, so any identifier the engine accepts works.

## How to ask

Use `@name` anywhere in the line, or the explicit `/consensflow:cf` router:

```text
@zeus What's the riskiest part of this design?                  # mention, anywhere in the line
/consensflow:cf @zeus What's the riskiest part of this design?  # explicit router
```

Pi intentionally matches Claude Code's slash-command surface: only `/consensflow:*` slash commands are registered; no unnamespaced shortcuts or per-agent slash commands. A stray `@token` that is not an agent is ignored and goes to the lead as normal text.

From the lead, **prefer the `cf_run_agent` tool.** Pass an optional `context` brief on top of the auto-included session handoff to focus the agent on exactly what you want assessed or done.

## Full command reference

Pi exposes the same ConsensFlow slash commands as Claude Code:

```text
/consensflow:cf [status|doctor|agents <…>|run @name <prompt>|ask @name <prompt>|@name <prompt>]
/consensflow:status
/consensflow:doctor
/consensflow:presets
/consensflow:agents [list|presets|add|show|remove|sync|add <…>]

@name <prompt>                                            # ask — mention anywhere in the line
```

## Tools available to the lead

- `cf_list_agents` — see who is configured.
- `cf_run_agent` — send one prompt to one agent. This is the preferred path when the lead consults on its own initiative.

`cf_run_agent` parameters the lead should know:

- `agent` — `@name` or `name`.
- `prompt` — the exact question/task for that agent.
- `context` — optional focused brief added on top of the automatic session handoff.
- `includeHandoff` — defaults to true; set false only when the agent should not see the current session snapshot.


- **Default and presets:** full permissions, not sandboxed — exactly like running the CLI yourself with every prompt pre-approved. They can plan, critique, explain, propose code, **and** edit files anywhere / run any command / reach the network.
- **After any run:** inspect what changed yourself (`git status`, `git diff`, relevant tests as needed) — consulting is no longer sandboxed, so a consult can modify files. Summarize what the agent changed, give your recommendation, and wait for user approval before keeping/building on/committing the changes unless the user pre-authorized that exact action.

## Invariants

- **One at a time.** Send to exactly one agent per call. Multiple leading `@mentions` are rejected; never fan out to several agents automatically. If the user names several, ask which one first, or ask one and wait for its answer before asking the next.
- **One-shot, no memory.** Each call is fresh. Continuity comes only from the handoff (re-sent each time), which already includes earlier `@agent` replies — so a later agent can build on an earlier one (cross-pollination). For a genuinely *independent* opinion, ask that agent **first**, before others have replied — otherwise its handoff carries the prior answers and colors it.
- **Always run in the foreground — never in the background.** Run agent calls in the FOREGROUND, NEVER in the background or detached; the live reasoning/tool/answer trail streams automatically (no flag needed) — the only exception is an explicit `--json` for machine output. Every agent run streams its normalized thinking / tool-call / answer events into the Pi UI as it goes (via `cf_run_agent`'s `onUpdate`); this is structural — there is no flag or agent decision that can suppress it.
- **The lead is always the decision-maker.** ConsensFlow routes a prompt and returns an answer; it never implements anything on its own. Acting on any answer goes through the gate above.
- **No automatic git context.** Agents receive only the handoff and the prompt — paste a diff or name the files when you want them assessed or changed.
- **No hidden workflows.** Do not assume ceremonies like spec review, implementation review, council, grill, or handoff-by-name. The skill routes one prompt to one agent; that is all.
