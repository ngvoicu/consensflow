---
name: consensflow
description: Use ConsensFlow inside Claude Code to consult one of the user's named AI agents — each a chosen model at a chosen effort, run one-shot by its own harness (claude, codex, pi, opencode) — for second opinions, design and code critique, questions, implementation help, or real work in the project. Activate whenever the user names an agent (e.g. "@zeus what do you think") AND whenever you would yourself benefit from an independent opinion on a risky or debatable decision — consulting is free, encouraged, and needs no permission. Acting on what an agent says, or keeping files it changed, is gated behind explicit user approval.
---

# ConsensFlow

ConsensFlow lets the lead (this Claude Code session) consult one named agent at a time. An agent is a model at a fixed effort, run one-shot by its own harness (claude / codex / opencode / pi) as an isolated subprocess: it receives your prompt, whatever you hand it, and nothing else — it answers once and does not persist between calls. It runs with full permissions — exactly like running that harness yourself with every approval already granted: it can read, edit files, run commands and reach the network, and it is not sandboxed to the project directory. Consulting an agent is like phoning an advisor who can also pick up a task. The lead stays the decision-maker, and ConsensFlow never accepts or keeps an agent's work on its own — inspect `git status` / `git diff` after any run before keeping changes.

## Reach for an advisor on your own

You do not need permission to consult, and you should not wait to be asked. A second opinion is cheap; a wrong decision the user has to unwind is not. Consult when:

- **you are about to finalize a non-trivial design or diff** — "review this approach; what is the riskiest assumption?";
- **a plan touches migrations, rollback, auth, money or data loss** — "poke holes in this: what breaks under load or on rollback?";
- **you are unsure between two designs** — ask one agent for a straight preference and a reason;
- **you have been stuck on the same failure twice** — a fresh reader with the same files often sees it immediately;
- **the user is about to act on your recommendation** and you have not had it checked by anyone.

Say who you asked and what they said. Do not consult in a loop: one agent, one question, then decide or ask the user.

## What agents can do

Use agents for all of these, one agent at a time. No preset is intrinsically review-only; the same agent can advise or do workspace work in the same run:

- **Advice / second opinion / design critique.** Ask an agent to inspect context, critique a plan, assess a pasted diff, identify risks, or suggest tests.
- **Doing work / code-writing help.** The same agent can implement, refactor, or run commands by default — it runs with full permissions and no extra flag. Treat it like a temporary helper: after the run, inspect `git status` / `git diff` and relevant tests, then ask the user before keeping or building on the changes unless they pre-authorized it.
- **Image generation.** `@pygmalion` (or any `kind=image` agent) uses **gpt-image-2** via the Codex backend / Codex CLI login. It receives the image prompt only — no session handoff — saves `image.png` in the ConsensFlow run dir under `~/.consensflow/workspaces/…`, and the lead can open/show that file with the Read tool. Optionally pass one or more **reference images** with `--image <path>` (repeatable) so gpt-image-2 edits/conditions on them — supply a file path (.png/.jpg/.jpeg/.webp/.gif); a pasted image with no path on disk can't be used.

## Spawning, in the user's words

The user will ask in plain language — "spawn diana as a good GDPR expert, give her our conversation, and ask her to review the export path". Compose that into one run:

- **who** → the agent's name (`@diana`);
- **as a …** → `--brief "You are reviewing this for GDPR: lawful basis, data minimisation, retention."`;
- **give it / don't give it our history** → write the relevant part of the conversation to a file and pass `--handoff-file`; leave it out when the task stands alone;
- **ask her to …** → the task itself, as the prompt.

The same four pieces work in every harness ConsensFlow supports, so the phrasing the user learns here is the phrasing that works in a cmux pane or in pi.

## How to run it

Everything the Claude Code lead does goes through the bundled CLI via the Bash tool. ConsensFlow never caps a run itself (runs are unbounded) — the only limit is your Bash tool timeout, so use a generous one for frontier models (often `600000` ms or more).

```bash
# Ask one agent (full permissions by default) in the foreground; the live trail streams automatically
cf run @zeus "What's the riskiest part of this design?"

# Add a focused brief on top of the automatic session handoff
cf run @zeus "Review the auth flow" --context "Focus on rollback and token expiry"

# Use a prompt file when the task is long or awkward to quote
cf run @zeus --prompt-file question.md

# Normalized thinking / tool / answer events stream live automatically; the parsed final answer is printed at the end too
cf run @zeus "Review this diff"

# Write is the default — no flag needed; the approval gate still applies afterward
cf run @builder "Make the minimal fix"


# Image generation with optional reference image(s) (--image is repeatable; image agents only)
cf run @pygmalion "A watercolor of this house at sunset" --image /tmp/house.png
cf run @pygmalion "Blend these into one scene" --image a.png --image b.jpg
```

Always run agent calls in the FOREGROUND, NEVER in the background; the live reasoning/tool/answer trail streams automatically — the only exception is an explicit `--json` for machine output.

Important run flags (flags may appear before or after the prompt/ref; `--prompt-file` may stand in for the prompt):

- `--brief "<what this run is for>"` — what you want from THIS spawn: "review this for GDPR: lawful basis, retention", "you are checking the migration for rollback safety". The agent is told nothing about itself otherwise — no persona is assigned for it — so the brief is where the framing goes. It leads the packet, above the task.
- `--context <note>` — a shorter note alongside the brief, for one run.
- `--handoff-file <file>` — the conversation, when the agent needs it. Nothing is attached behind your back: write the relevant part to a file and pass it, or leave it out and the agent sees only the task.
- `--image <path>` — reference image for an `image` agent; repeatable for multiple references. Ignored by text agents.
- `--json` — print full run metadata instead of just the human answer.

The conversation is yours to hand over: write the part that matters to a file and pass `--handoff-file`. Nothing is stashed or attached automatically — an agent sees the brief, the task, and whatever you gave it.

Artifacts land in the workspace's run dir under `~/.consensflow/workspaces/…` (`packet.md`, `stdout.txt`, `stderr.txt`, `result.json`, `transcript.md`) — never inside the project. `packet.md` is byte-for-byte what the agent received; `transcript.md` is the durable event-trail backstop.

## The two rules that matter most

These two rules are the heart of the skill. They pull in opposite directions on purpose: consulting is cheap and encouraged; acting on the result is gated.

### 1. Consulting is free — and encouraged

The lead may, and should, reach for an agent on its own initiative, with NO user permission needed merely to consult. Treat an agent consultation as a natural reflex, not a rare ceremony — before finalizing a non-trivial design or diff, consider getting another take. Just run the CLI; you do not ask the user "may I consult X?" first. Good moments, with the kind of prompt to send:

- **Before finalizing a non-trivial design or diff** — `@zeus` "Review this approach; call out the riskiest assumption and anything you'd do differently."
- **To stress-test a plan** — `@hyperion` "Poke holes in this migration plan — what breaks under load or on rollback?"
- **To surface unknowns before building** — `@gaia` "What questions should I answer before I start implementing this?"
- **For a quick gut-check or tie-breaker** — `@nike` "Does this error handling read as correct to you — yes/no with one reason?"
- **For a focused diff/task check** — run `git diff` yourself and paste the relevant parts into the prompt or `--context` brief.

### 2. Acting on the output is GATED — never without asking

The lead MUST NOT apply, merge, commit, adopt, integrate, or otherwise act on an agent's response — and MUST NOT keep or extend any files an agent edited — without first surfacing it to the user and getting explicit approval. This is a hard rule, not a preference.

Before acting, the lead MUST present:

- a concise **summary** of what the agent said or did, and
- the **lead's own recommendation** (accept / accept-with-changes / reject, and why).

Then wait for the user to approve.

This gate covers BOTH cases equally:

- **(a) Advice in a text response.** Do not implement, refactor toward, or commit to an agent's suggestion until the user approves it.
- **(b) Real file changes by an agent.** Any agent may have edited files or run commands in the workspace — that's the default. Do not treat that work as accepted: inspect what changed yourself (for example `git status` / `git diff` in the relevant repo), then surface a summary + recommendation and get approval before keeping, building on, or committing it. If the user rejects it, revert it.

**The only exception:** the user has already explicitly told the lead to proceed — e.g. "get Zeus's take and apply what makes sense," or "run the builder and commit it." Pre-authorization scoped to that request stands in for the approval; do not re-ask. Absent such an instruction, never act on an agent's output on your own.

### 3. When it answers, stop and talk to the user

An agent's reply ends your turn, it does not continue it. Report what came back — attributed and faithful, not summarized away — say what you make of it, and then wait. Do not start implementing it, do not spawn a second agent for a tie-break, do not pick your plan back up where you left it. An answer the user has not read is not a decision the user has made.

This holds even when the answer agrees with you, and even when acting on it would change nothing on disk: the point of consulting is that the user hears another voice, and they cannot hear it if you have already moved on.

Do / Never, in one line each:

- **Do** consult an agent whenever a second opinion would help — no permission needed.
- **Never** apply, commit, or keep an agent's advice or file changes without the user's go-ahead, unless the user pre-authorized it.

In short: ask freely, apply only with a green light.

## How agents are created

Agents are configured in the shared roster `~/.consensflow/agents.json` (set up once, use from any project, Claude Code, and the Pi sibling). There are no per-tool config roots. Agents come from curated presets or fully custom definitions:

**The names below are a menu, not your roster.** None of them exists until it is added. Your actual agents are the ones `agents list` prints — and the line at the top of this session already named them. `@zeus` and friends appear throughout this skill only as placeholders in examples; substitute a name you actually have, and if the user asks for one that is not on the roster, say so and offer to add it rather than guessing a substitute.

```bash
cf agents presets                    # list built-in presets
cf agents add zeus                   # add a preset → @zeus
cf agents add endymion               # Pi-backed Kimi K3 → @endymion
cf agents add all                    # add every preset
cf agents add zeus --name Deepreview # preset backend, renamed → @deepreview
cf agents add --name Builder --kind codex --model gpt-5.6-sol --effort high   # fully custom; read-write like every agent
```

Presets run read-write like any agent; the same model+effort family exists on every engine that runs it:

- **Fable 5** (Anthropic's top model — use for the questions that really matter): `@calliope`/`@clio`/`@euterpe`/`@thalia` (Claude Code max/xhigh/high/medium), `@orpheus`/`@linus`/`@erato` (Pi xhigh/high/medium, Anthropic auth), `@saga`/`@gunnlod`/`@kvasir` (OpenCode xhigh/high/medium via OpenRouter).
- **Sonnet 5**: `@hermod` (Claude Code, max effort).
- **Opus 5**: `@zeus`/`@apollo`/`@artemis` (Claude Code max/xhigh/medium), `@kronos`/`@atlas` (Pi xhigh/medium, Anthropic auth), `@baldr`/`@vali` (OpenCode xhigh/medium via OpenRouter).
- **GPT 5.6** (three variants: Sol flagship, Terra balanced, Luna fast): `@hyperion`/`@phoebus` (Codex Sol ultra/xhigh), `@gaia` (Codex Terra xhigh), `@diana` (Codex Luna xhigh), `@aether`/`@rhea`/`@phoebe` (Pi Sol/Terra/Luna xhigh, same ChatGPT login), `@sunna`/`@jord`/`@bil` (OpenCode Sol/Terra/Luna xhigh via OpenRouter).
- **Deep open-weights**: Kimi K3 — `@endymion` (Pi, xhigh thinking), `@mani` (OpenCode). K2.7 Code was retired in 1.9.0.
- **Fast/cheap tier** (quick gut-checks): `@nike`/`@sif` (Gemini 3.7 Flash on Pi/OpenCode), `@zephyros`/`@freya` (DeepSeek V4 Flash 0731 on Pi/OpenCode).
- **Model zoo** (same OpenRouter models on two engines; Greek = pi, Norse = opencode): DeepSeek V4 Pro 0813 `@hades`/`@odin`, Gemini 3.1 Pro `@helios`/`@heimdall`, Grok 4.6 `@ares`/`@thor`, Qwen3.8 Max `@hephaestus`/`@tyr`, MiniMax M3 `@metis`/`@mimir`, GLM 5.3 `@prometheus` (pi only).
- **Image**: `@pygmalion` (kind=image) generates a picture with gpt-image-2 via the Codex CLI login (`codex login`) — prompt-only (no handoff), optional `--image <path>` reference(s), PNG saved as `image.png` in the run dir; open it with the Read tool to view or show it.

Model and effort strings pass through to the engine verbatim, so any identifier the engine accepts works.

## Full command reference for the lead

Use the CLI directly from Bash:

```bash
cf status
cf doctor
cf agents list
cf agents presets
cf agents add <preset> [--name <name>] [--cwd <subdir>]
cf agents add all
cf agents add --name <name> --kind <pi|claude-code|codex|opencode|image> --model <model> [--effort <e>|--thinking <t>] [--cwd <subdir>]
cf agents show @name
cf agents remove @name
cf agents sync [--dry-run]   # re-resolve preset-backed agents against the current catalog
cf run @name <prompt> [--prompt-file <file>] [--context <note>] [--no-handoff] [--image <path> …] [--json]
```

One user-facing slash command wraps that CLI: `/consensflow <args>` — it passes whatever follows straight through (`/consensflow status`, `/consensflow agents list`, `/consensflow @diana <prompt>`).


- **Default and presets:** full permissions, not sandboxed. Agents can read, plan, critique, explain, propose code, edit files anywhere, run any command, and use the network — exactly like running the CLI yourself with every prompt pre-approved.
- **There is no permission flag to pass.** No tier, policy or `--tools` value changes anything: every run already has everything. What protects the user is the approval gate below on *keeping* the work, not a fence around the run.
- **After any run:** run your own inspection (`git status`, `git diff`, relevant tests as needed), summarize what the agent changed, give your recommendation, and wait for user approval before keeping/building on/committing the changes unless the user pre-authorized that exact action.

## How the user asks

The request arrives in words, and it is yours to act on — nothing routes it for you. "Ask zeus what breaks on rollback", "@zeus what's the riskiest part of this design?", "spawn diana as a GDPR reviewer and give her our conversation": all of them mean compose one `cf run` and report what comes back.

A name you do not recognise is not a spawn — a stray `@token` like `@types/node` is ordinary text. If the user names an agent that is not on the roster, say so and offer to add it rather than substituting a different one.

The `/consensflow` slash command is the explicit form (`/consensflow @name <prompt>`, `/consensflow doctor`, …) for driving a run by hand.

## Invariants

- **One at a time.** Send to exactly one agent per call. Never fan out to several agents automatically. If the user names several, ask which one first, or ask one and wait for its answer before asking the next.
- **One-shot, no memory.** Each call is fresh. Continuity comes only from the handoff (re-sent each time), which already includes earlier agent replies — so a later agent can build on an earlier one (cross-pollination). For a genuinely *independent* opinion, ask that agent **first**, before others have replied — otherwise its handoff carries the prior answers and colors it.
- **Foreground is non-optional.** Always run agent calls in the FOREGROUND, NEVER in the background or detached; the live reasoning/tool/answer trail streams automatically (no flag needed). The lead must not swap it for `--json` or summarize the streamed trail away. The one exception is an explicit user request for JSON output.
- **The lead is always the decision-maker.** ConsensFlow routes a prompt and returns an answer; it never implements anything on its own. Acting on any answer goes through the gate above.
- **No automatic git context.** Agents receive only the handoff and the prompt — paste a diff or name the files when you want them assessed or changed.
- **No hidden workflows.** Do not assume ceremonies like spec review, implementation review, council, grill, or handoff-by-name. The skill routes one prompt to one agent; that is all.
- **No nesting.** Agent subprocesses run with `CONSENSFLOW_CHILD=1` and must not start their own ConsensFlow runs.
