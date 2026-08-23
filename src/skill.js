import { HARNESSES } from './roster.js'
/**
 * Generates the consensflow SKILL.md from the roster.
 *
 * The prose is the product: this template is the hand-written v0 skill that
 * was live-proven on claude and codex (2026-08-19) before any of this code
 * existed. Only the roster table and the description's name list vary.
 *
 * The `env -u …_API_KEY` prefixes are deliberate: they keep subscription
 * logins from silently switching to API-key billing (v1's dropEnv, carried
 * as prose). The danger flags appear only on rows the user explicitly stored
 * as full-auto — the table is copy-paste commands a lead will run verbatim.
 */

/**
 * The line that spawns this agent — the same line for every one of them.
 *
 * Each harness used to get its own hand-built command with its own flags and
 * billing guards, which made the skill a lookup table and the UI a phrasebook.
 * One verb replaced them: `cf run` builds the packet, applies the guards and
 * streams the run, whichever harness is behind the name.
 */
export function agentCommand(p) {
  return `cf run @${p.name} "<task>"`
}

function row(p) {
  const traits = [p.description, p.effort ? `${p.effort} effort` : null].filter(Boolean).join('; ')
  const label = traits.length > 0 ? `**${p.name}** — ${traits}` : `**${p.name}**`
  return `| ${label} | ${p.harness} | \`${p.model}\` |`
}

/**
 * The skill, generated from the roster — and, in cmux mode, from the fact that
 * panes exist there.
 *
 * `options.mode` only ever ADDS the pane step, and only for `cmux`. ConsensFlow
 * still never drives a pane itself: it says what to run, cmux's own skills say
 * how to place it, and the lead composes the two. That is the same split that
 * kept `cf run` a plain subprocess with an exit code instead of a terminal to
 * be scraped — the v2 lesson this project was rebuilt to avoid.
 */
export function generateSkill(agents, options = {}) {
  const supported = agents.filter((p) => HARNESSES.includes(p.harness))
  if (supported.length === 0) {
    throw new Error('empty roster: add an agent before generating the skill')
  }
  const names = supported.map((p) => p.name).join(', ')
  const inCmux = options.mode === 'cmux'

  return `---
name: consensflow
description: Consult one of the user's named AI agents — ${names} — each a real coding-harness CLI (claude, codex, pi, opencode) run one-shot in the current directory. Use whenever the user says "ask <name> …", "what does <name> think", "consult <name>", "get a second opinion from <name>", or names any agent — and also when you yourself want an independent second opinion on a risky or debatable decision.
---

# ConsensFlow agents

The user keeps a roster of named AI agents. Each one is a model at a fixed
effort, run one-shot by its own harness (claude, codex, pi, opencode) — a
separately installed CLI. Consulting an agent means running its command below
with your question as the final argument. It runs in **your current working
directory** and reads the project's files itself, so you need not paste file
contents — but it cannot see this conversation, so the question has to carry
the context that matters.

## Reach for an advisor on your own

You do not need permission to consult, and you should not wait to be asked. A
second opinion is cheap; a wrong decision the user has to unwind is not.
Consult when:

- **you are about to finalize a non-trivial design or diff** — "review this
  approach; what is the riskiest assumption?";
- **a plan touches migrations, rollback, auth, money or data loss** — "poke
  holes in this: what breaks under load or on rollback?";
- **you are unsure between two designs** — ask for a straight preference and
  a reason;
- **you have been stuck on the same failure twice** — a fresh reader with the
  same files often sees it immediately;
- **the user is about to act on your recommendation** and nobody has checked
  it.

Say who you asked and what they said. Do not consult in a loop: one agent, one
question, then decide or ask the user.

## How to consult

1. Pick the agent the user named (or choose one yourself when you want
   a second opinion).
2. Compose the question: one or two sentences of task context, then the
   concrete question. Name specific files with relative paths when relevant.
3. Spawn it from the project directory:

   \`\`\`bash
   cf run @<name> "<task>"
   \`\`\`
${
  inCmux
    ? `
   **In that conversation's pane, always** — never in this one. See "One pane
   per conversation" below for the three commands that open it; do not go
   exploring cmux's CLI, the recipe is written out for you.
`
    : ''
}
   Flags, all optional and combinable:

   - \`--brief "<what this run is for>"\` — what you want from THIS spawn:
     "review this for GDPR: lawful basis, retention", "you are checking the
     migration for rollback safety". The agent is told nothing about itself
     otherwise, so the brief is where the framing goes.
   - \`--handoff-file <file>\` — your conversation so far, when the agent needs
     it. You are the one holding it: write the relevant part to a file and
     pass it.
   - \`--no-handoff\` — spawn with the task alone. Only matters when a harness
     has stashed the conversation for you (the Claude Code and pi
     integrations do); here it is the default, and this flag makes it
     explicit.
   - \`--context "<note>"\` — a short brief-alongside for one run.
   - \`--prompt-file <file>\` — when the task is long.
   - \`--image <path>\` — reference pictures for an image agent, repeatable.

   Turns can take minutes at high effort — use a generous timeout (10+
   minutes for max/ultra). The thinking streams as it goes.
4. Report the answer to the user **verbatim or faithfully summarized, and
   attributed** ("hyperion says: …"). Never present an agent's answer as
   your own.

## Rules

- **One agent at a time.** Wait for one answer before asking another.
- **Advice is free; acting is gated.** Never apply an agent's suggested
  changes, or keep files it created, without the user's explicit approval —
  unless the user already authorized it in this conversation.
- **Bring the answer back before anything else.** When an agent replies,
  stop. Report what it said — attributed and faithful, not summarized away —
  and add what you make of it. Then wait. Do not start implementing, do not
  spawn a second agent, do not resume your plan until the user has weighed in.
  An answer they have not read is not a decision they have made.
- **Do not retry a slow agent with a different one** unless the command
  itself failed. Slow usually means thinking.

## Roster

| Agent | Harness | Model |
|---|---|---|
${supported.map(row).join('\n')}

Every one of them is spawned the same way — \`cf run @<name> "<task>"\` — so
picking an agent is a question of who you want, not of what to type. The
command carries the billing guards for you: a run never switches a
subscription login to API-key billing.

${
  inCmux
    ? `## One pane per conversation

A consult here is a **conversation**, not a one-shot. The first \`cf run\` to an
agent starts one and prints its name — \`ember-ridge\`, \`amber-moss\`. Ask that
agent again and it continues: the harness resumes its own session, so the agent
remembers what you already discussed and the provider's cache is still warm.

Give each conversation its own pane. Three commands, in this order — you do not
need to explore cmux's CLI, and you must not run the consult in this pane:

\`\`\`bash
# 1. a pane beside you, without stealing focus. Prints: OK surface:NN pane:NN
CMUX_QUIET=1 cmux new-pane --type terminal --direction right --focus false

# 2. send the consult there. The trailing newline is what runs it.
CMUX_QUIET=1 cmux send --surface surface:NN 'cf run @<name> "<task>" --brief "<why>"'$'\\n'

# 3. once cf prints "conversation: <name>", title the tab to match
CMUX_QUIET=1 cmux rename-tab --surface surface:NN '<name>'
\`\`\`

Then **read the answer with \`cf last <name>\` from your own pane** — do not
scrape the other pane's screen. The pane is for the user to watch; the run
directory is what you read. Screen text is not an answer, it is a picture of
one.

Later turns in that conversation go to the same pane: reuse its surface id, or
find it by the tab name. Two conversations mean two panes, so their answers
never tangle.

\`\`\`bash
cf run @<name> "<task>"                     # continues that agent's conversation here
cf run @<name> "<task>" --new               # starts a fresh one, prints its name
cf run @<name> "<task>" --session <name>    # continues a specific one
cf sessions                                 # what is alive in this workspace
cf last <name>                              # read an answer from your own pane
\`\`\`

Start a **new** conversation when the subject genuinely changes. Continuing one
carries its whole history into every later turn, which is what makes the agent
useful — and what makes an unrelated question expensive.

If you cannot open a pane — no workspace, cmux is not running — say so and run
the consult here rather than silently doing something the user did not ask for.

The cmux commands above are quoted so you need not go looking for them; they
are cmux's, not ours, and your cmux skills are the authority if they have moved.

This skill defines *what* to run; the cmux skills define pane control. Neither
one reaches into the other.

`
    : ''
}## Roster maintenance

The roster above is generated by ConsensFlow. To change it, the user runs
\`cf agent …\` or \`cf ui\` — never edit this file by hand; it will be
regenerated.

If an agent the user names is missing from the table, the roster may
have changed since this file was generated (it is shared with other
ConsensFlow tools): run \`cf skills update\`, then re-read this file.
`
}
