import { HARNESSES } from './roster.js'
/**
 * Generates the consensflow SKILL.md from the roster.
 *
 * The prose is the product: this template is the hand-written v0 skill that
 * was live-proven on claude and codex (2026-08-19) before any of this code
 * existed. Only the roster table and the description's name list vary.
 *
 * ConsensFlow has one shape now: the app owns the panes (standalone). There
 * are no modes, so this generator takes no mode option — anything still
 * passing one gets the same text. The danger flags appear only on rows the
 * user explicitly stored as full-auto — the table is copy-paste commands a
 * lead will run verbatim.
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
 * The skill, generated from the roster.
 *
 * One text for every lead: the app opens every pane itself, so there is no
 * pane step here to add or vary. ConsensFlow never drives a pane on the
 * lead's behalf either — it says what to run, the app places it, and the
 * lead composes the two. That is the same split that kept `cf run` a plain
 * request with a printed conversation name instead of a terminal to be
 * scraped — the v2 lesson this project was rebuilt to avoid.
 */
export function generateSkill(agents, _options = {}) {
  const supported = agents.filter((p) => HARNESSES.includes(p.harness))
  if (supported.length === 0) {
    throw new Error('empty roster: add an agent before generating the skill')
  }
  const names = supported.map((p) => p.name).join(', ')

  // The description is the only part of this file a lead reads before deciding
  // whether to open the rest. It must not sound complete and must not hand
  // out a runnable command: a lead that read "run one-shot in the current
  // directory" concluded there was nothing further to learn and ran the
  // consult in its own pane (2026-08-24).
  const shape =
    "working in the current directory. A consult here opens a conversation in the app's own window — the app owns the panes — so read this skill before starting one; the exact commands are in it."

  return `---
name: consensflow
description: Consult one of the user's named AI agents — ${names} — each a real coding-harness CLI (claude, codex, pi, opencode, kimi) ${shape} Use whenever the user says "ask <name> …", "what does <name> think", "consult <name>", "get a second opinion from <name>", or names any agent — and also when you yourself want an independent second opinion on a risky or debatable decision.
---

# ConsensFlow agents

The user keeps a roster of named AI agents. Each one is a model at a fixed
effort, run by its own harness (claude, codex, pi, opencode,
kimi) — a separately installed CLI. Consulting an agent means running its command below
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

Say who you asked and what they said. Independent questions can run in
parallel in their own conversations; a follow-up belongs to the conversation
that already holds its context.

## How to consult

There are three acts: consult, follow up, read.

1. Pick the agent the user named (or choose one yourself when you want
   a second opinion).
2. Compose the question: one or two sentences of task context, then the
   concrete question. Name specific files with relative paths when relevant.
3. Consult it from your own pane in the app:

    \`\`\`bash
    cf run @<name> "<task>"
    \`\`\`

   Today's continuation rule, with no pane of your own to open: run it bare
   and the agent's conversation with you continues; pass \`--new\` for an
   independent task and the app starts a fresh conversation; pass
   \`--session <name>\` to name one explicitly. The app prints the name it
   minted:

    \`\`\`bash
    cf run @<name> "<task>" --new            # an independent task: a fresh conversation
    cf run @<name> "<task>" --session <name> # a specific one, by name
    # conversation: <name> (new) — pane <id>
    \`\`\`

   A task that leans on a conversation stays in it; only an independent one
   gets a new conversation — continue by default, unsure means continue. An
   independent task is one you could hand a stranger in full, naming its own
   files, without a word about what that conversation said.

   Flags, all optional and combinable:

    - \`--brief "<what this run is for>"\` — what you want from THIS run:
      "review this for GDPR: lawful basis, retention", "you are checking the
      migration for rollback safety". The agent is told nothing about itself
      otherwise, so the brief is where the framing goes.
    - \`--handoff-file <file>\` — your conversation so far, when the agent needs
      it. You are the one holding it: write the relevant part to a file and
      pass it.
    - \`--context "<note>"\` — a short brief-alongside for one run.
    - \`--prompt-file <file>\` — when the task is long. It IS the task: pass it
      INSTEAD of the quoted one, never beside it, and put your framing in
      \`--brief\`. Both together is refused, because the file would otherwise
      replace what you quoted without a word.
    - \`--image <path>\` — reference pictures for an image agent, repeatable.

   Turns can take minutes at high effort — use a generous timeout (10+
    minutes for max). The thinking streams as it goes.
4. Report the answer to the user **verbatim or faithfully summarized, and
   attributed** ("hyperion says: …"). Never present an agent's answer as
   your own.

## Follow up in the same conversation

A follow-up is the same question you would ask a colleague who already read
the files: send it into the conversation that holds the context.

\`\`\`bash
cf say <name> "<your follow-up, in plain words>"
\`\`\`

Look before you send: run \`cf results <name>\` first — the
conversation may have answered without you, and a follow-up composed against a
stale view asks the wrong question.

## Read what arrived

Reading a conversation and adding to it are different acts. "What did he
say?", "did he reply?" ask you to READ: run \`cf results <name>\` to discover
its completed results, then \`cf read <name>\` to read the oldest unread one
whole. Send nothing. Asking the agent again invents a new answer instead of
finding the one that already exists.

A completed result is read WHOLE from the top, never from the end. An answer
that arrives in your pane is the complete answer — read all of it, starting
at the first line, before you report or act on any of it. A line naming
\`cf read <id>\` means the answer arrived as a file: run each part and read
its complete output in full before anything else. Truncation is never the
whole result: a clipped fragment or an unfinished answer is not a complete
result, so read every part until the answer is whole — the first part prints
its immutable delivery id, and every further part uses that id.

\`\`\`bash
cf results                   # every conversation's completed results, with status and preview
cf results <name>            # one conversation's completed results
cf results @<agent>          # that agent's conversations
cf read <name>               # the oldest unread completed result, first part
cf read <name> --answer <id> # one specific completed result, first part
cf read <id> --part 2        # every further part uses the delivery id the first part printed
\`\`\`

A result read covers that result only: it says nothing about the discussion
around it, and the omitted discussion stays unread.

## Send and return, never wait

After a consult or a follow-up, report what is running and in which
conversation, then take the user's next message. Under \`auto\` the daemon
delivers every completed answer into your pane on its own — read what
arrives. Under \`manual\` there is no automatic delivery, but the answers are
still there whenever the authorized task needs them: invoke \`cf results\` to
discover completed results and \`cf read\` to read them whole. Either way
you do not sit out the answer: polling is wrong — \`cf results\`
in a loop or \`cf sessions\` every few seconds burns the user's attention and
answers nothing sooner.

## The pane's input line belongs to the human

The lead pane is a native terminal, and what the human typed into it is
opaque to you: never assume the input line is empty. A manual \`cf read\`
never touches terminal input — it only prints into your own tool result.
When the line must be clear, the user sends or erases the input, and a
human-only Resume replies is what confirms it is empty. Nothing clears it
automatically from the transcript — do not promise that it does.

## Rules

- **Advice is free; acting is gated.** Never apply an agent's suggested
  changes, or keep files it created, without the user's explicit approval —
  unless the user already authorized it in this conversation.
- **Bring the answer back before anything else.** When an agent replies,
  stop. Report what it said — attributed and faithful, not summarized away —
  and add what you make of it.
- **Do not retry a slow agent with a different one** unless the command
  itself failed. Slow usually means thinking.
- **A policy the human set is never changed.** Delivery policy belongs to the
  human on the page: never change it, never work around it, and never treat a
  quiet conversation as permission to flip it.

## Roster

| Agent | Harness | Model |
|---|---|---|
${supported.map(row).join('\n')}

Every one of them is consulted the same way — \`cf run @<name> "<task>"\` — so
picking an agent is a question of who you want, not of what to type. The
command carries the billing guards for you: a run never switches a
subscription login to API-key billing.

## Roster maintenance

The roster above is generated by ConsensFlow. To change it, the user runs
\`cf agent …\` or \`cf ui\` — never edit this file by hand; it will be
regenerated.

If an agent the user names is missing from the table, the roster may
have changed since this file was generated (it is shared with other
ConsensFlow tools): run \`cf skills update\`, then re-read this file.
`
}
