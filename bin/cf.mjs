#!/usr/bin/env node
/**
 * The `cf` executable — ConsensFlow v3.
 *
 * v3 is skills-first: there is no delegation engine here. `cf` manages the
 * roster of named agents, generates the consensflow skill from it, and
 * installs/updates that skill into every coding
 * harness on the machine (claude, codex, pi, opencode). The skill teaches the
 * harnesses everything else.
 */
import { spawn } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { answers as harnessAnswers } from '../hosts/lib/completion.js'
import {
  discoverCodexSession,
  discoverKimiSession,
  discoverOpencodeSession,
} from '../hosts/lib/harness-transcript.js'
import { renderImageRun, runImageAgent } from '../hosts/lib/image-run.js'
import { createPacket, createWindowSeed } from '../hosts/lib/packets.js'
import { childEnv, interactiveResume, interactiveStart, runAgent } from '../hosts/lib/runners.js'
import { openingLineCarriesNonce, TURNS_EXAMINED } from '../hosts/lib/session-binding.js'
import { runsRoot } from '../hosts/lib/state.js'
import { loadThreads } from '../hosts/lib/threads.js'
import { renderEvent } from '../hosts/lib/transcript-events.js'
import { CATALOG, catalogEntry } from '../src/catalog.js'
import { launchConfiguration } from '../src/channels.js'
import { detectHarnesses } from '../src/harnesses.js'
import { staleClaudeHooks } from '../src/host-payloads.js'
import {
  installEverywhere,
  installSkill,
  resetEverything,
  resetPreview,
  skillsStatus,
  skillsSummary,
  syncCmuxSkills,
  turnOff,
  uninstallSkills,
} from '../src/install.js'
import { appRequester } from '../src/requester.js'
import {
  addAgent,
  agentRow,
  configRoot,
  editAgent,
  listAgents,
  migrateStateRoot,
  removeAgent,
  syncAgents,
} from '../src/roster.js'
import { generateSkill } from '../src/skill.js'
import {
  healSkillIfStale,
  refreshInstalledSkill as refreshSkill,
  retireSkillFromNativeHosts,
  skillGaps,
  skillTargets,
  staleSkills,
} from '../src/sync.js'
import { terminalRuntime } from '../src/terminal.js'

// `cf … | head` closes our stdout mid-stream; dying with an EPIPE stack for
// that is a crash where a quiet exit is the whole contract of a CLI.
//
// The editor is the one verb that owns durable state, so it installs a drain
// here: a broken pipe must not cut a delivery's outcome off before it reaches
// disk. Every other verb has nothing to finish and exits as it always did.
const owner = { drain: null }
for (const stream of [process.stdout, process.stderr]) {
  stream.on('error', (error) => {
    if (error.code !== 'EPIPE') throw error
    if (owner.drain === null) process.exit(0)
    else void owner.drain()
  })
}

const HERE = dirname(fileURLToPath(import.meta.url))
const PKG = JSON.parse(readFileSync(join(HERE, '..', 'package.json'), 'utf8'))
const env = process.env

const USAGE = `consensflow ${PKG.version}

Usage: cf <command> [options]

  setup [--all] [--force]                     Install the CLI and generated skill
  run <@agent> "<task>"                       Ask a worker in a ConsensFlow app pane
    [--brief <purpose>] [--context <note>]
    [--prompt-file <file>] [--handoff-file <file>] [--no-handoff]
    [--new | --session <conversation>]        Continue by default; --new mints a name
    [--notify auto|manual] [--image <path>]
  say <conversation> "<words>"               Continue in that conversation's app pane
  attach <@agent|conversation>                Reopen a conversation through the app
  read <conversation|delivery id>          Read one completed result whole
    [--answer <id>] [--part <k>]
  results [conversation|@agent] [--json]    List completed worker results
  sessions [--json]                          List conversations in this workspace
  last <conversation|@agent> [--json]         Read the last recorded answer
  catalog [--harness <h>] [--json]            List available agent presets
  agent add <name>                           Add a catalog agent
    [--harness <h>] [--model <m>] [--effort <e>] [--description <d>]
  agent list [--json]
  agent edit <name> [--model <m>] [--effort <e>] [--description <d>]
  agent remove <name>
  agent sync [<name>] [--dry-run]             Refresh catalog-owned agent fields
  skills install [--all]                     Install the generated roster skill
  skills update [--force]                    Refresh skills and retire owned leftovers
  skills status                             Inspect installed skills
  skills uninstall [--force]                Remove manifest-owned skills
  ui [--json] [--no-open]                     Open the local roster editor
  doctor                                    Inspect runtime, roster and skill installation
  off [--force]                             Remove owned installation; keep agents and history
  reset [--yes]                             Remove installation, agents and local app history

Run, say, attach, read and results need a pane opened by ConsensFlow.
The app owns conversation launches, delivery and read marks.
Every roster change refreshes the generated skill. Unowned skill files stay untouched.
`

function out(text) {
  process.stdout.write(`${text}\n`)
}

function fail(message) {
  process.stderr.write(`cf: ${message}\n`)
  process.exitCode = 1
}

/**
 * Something worth saying that is not this command's verdict.
 *
 * A pane whose binding failed still ran its window, and the window is what
 * the exit status is about; the binding gets the reader's attention without
 * taking the process's answer away from the thing that actually happened.
 */
function warn(message) {
  process.stderr.write(`cf: ${message}\n`)
}

const NATIVE_OWNER = {
  claude: 'the consensflow-cc plugin',
  pi: 'the consensflow-pi extension',
}

/** Says out loud where the generated skill was deliberately not installed. */
function reportNativeHosts(env, all) {
  if (all) return
  // An upgrade can inherit copies installed before the host had its own.
  for (const row of retireSkillFromNativeHosts(env)) {
    out(
      row.action === 'retired'
        ? `retired          ${row.path}`
        : `kept (you edited it)  ${row.path}`,
    )
  }
  for (const harness of detectHarnesses(env).filter((a) => a.native === true)) {
    out(
      `${harness.id}: left alone — ${NATIVE_OWNER[harness.id] ?? 'its own integration'} already provides a consensflow skill (--all to install ours too)`,
    )
  }
}

/**
 * ConsensFlow ships one skill — its own. This takes back what the cloning
 * era installed: cmux-sourced files and the checkout cache. Local disk work
 * only; it cannot fail on the network because it never touches one.
 */
function syncCmux(env, values) {
  printReport(syncCmuxSkills(env, { force: values.force }).report)
}

function printReport(report) {
  for (const row of report) {
    out(`${row.action.padEnd(16)} ${row.path}`)
  }
}

/**
 * A catalog name is a whole agent: `cf agent add zeus` needs no
 * flags. Anything passed explicitly wins over the catalog entry, and a name
 * nobody knows still needs a harness and a model.
 */
function resolveAdd(name, values) {
  const entry = catalogEntry(name)
  if (entry === undefined && (values.harness === undefined || values.model === undefined)) {
    throw new Error(
      `${name} is not in the catalog, so it needs --harness and --model (see \`cf catalog\`)`,
    )
  }
  // Provenance only when the catalog actually decided the agent: an
  // explicit --model or --effort makes this the user's own definition, and a
  // later sync must not drag it back to the preset.
  const pinned = values.model !== undefined || values.effort !== undefined
  return {
    name,
    harness: values.harness ?? entry?.harness,
    model: values.model ?? entry?.model,
    effort: values.effort ?? entry?.effort,
    description: values.description ?? entry?.description,
    ...(entry !== undefined && !pinned ? { preset: entry.preset } : {}),
  }
}

/**
 * Which conversation a verb means — `attach` and `last` both answer it the
 * same way, and each used to carry its own copy of this. (`cf results`
 * filters the app's own listing instead: the tab it lists is the lead's,
 * never this workspace's files.)
 *
 * Joining is deliberately NOT lead-scoped (spawning is): whoever is reading
 * gets the newest conversation when they name nothing, that agent's newest for
 * `@agent`, and exactly the one they named otherwise. `record` is undefined
 * when nothing matches — the verbs keep their own error text.
 */
function pickConversation(threads, asked) {
  const byRecency = (names) =>
    [...names].sort((a, b) =>
      String(threads[b].lastRunAt ?? '').localeCompare(String(threads[a].lastRunAt ?? '')),
    )[0]
  const name =
    asked.length === 0
      ? byRecency(Object.keys(threads))
      : asked.startsWith('@')
        ? byRecency(Object.keys(threads).filter((key) => threads[key].agent === asked.slice(1)))
        : asked
  return { name, record: name === undefined ? undefined : threads[name] }
}

/**
 * Why nothing matched, said so that the next attempt works.
 *
 * `@name` is an agent and a bare name is a conversation, everywhere in this
 * CLI — which makes `cf last triton` a one-character mistake, and one the
 * answer used to leave the reader to spot in a list. The verbs that resolve
 * a name share one wording now ("no conversation X" against "no
 * conversation named X"), and it names the missing `@`.
 */
function noConversationHere(asked, names, env) {
  // Emptiness first, whatever was asked for: a workspace with nothing in it
  // wants the sentence that starts one, not a report about the name.
  if (names.length === 0) return 'no conversations here yet — `cf run @name "<task>"` starts one'
  if (asked.startsWith('@')) {
    return `no conversation with ${asked} here; you have: ${names.join(', ')}`
  }
  if (agentRow(asked, env) !== undefined) {
    return `no conversation named ${JSON.stringify(asked)} here — ${asked} is an agent, so \`@${asked}\` takes its most recent one; conversations here: ${names.join(', ')}`
  }
  return `no conversation named ${JSON.stringify(asked)} here; you have: ${names.join(', ')}`
}

const DISCOVER = { opencode: discoverOpencodeSession, codex: discoverCodexSession }

const cwdOf = () => process.cwd()

// --- standalone: ConsensFlow's own app owns the panes -------------------------

/**
 * The app's authority in this process, or null when there is none.
 *
 * `src/launch.js` hands a lead pane `CONSENSFLOW_APP` plus a tab-scoped
 * token, and a `--in-pane` controller `CONSENSFLOW_APP` plus a single-use
 * ticket. This is the ONLY place `cf` reads them; everything below takes
 * what it needs as an argument, which is what `src/requester.js` is for.
 */
function appHere() {
  const url = env.CONSENSFLOW_APP
  if (typeof url !== 'string' || url.trim().length === 0) return null
  return {
    url,
    token: env.CONSENSFLOW_APP_TOKEN,
    tab: env.CONSENSFLOW_TAB,
    pane: env.CONSENSFLOW_PANE_ID,
    ticket: env.CONSENSFLOW_LAUNCH,
    lead: env.CONSENSFLOW_LEAD_ID,
  }
}

/**
 * The app's launch evidence, checked the same way wherever it arrives.
 *
 * `cf run --in-pane` and `cf attach --in-pane` are two doors into one
 * launch, and a door that skips the check is the check. Answers the single
 * flag's value, or null once it has said why it refuses.
 */
function launchEvidenceOf(values) {
  const given = [
    ...(values.launch === undefined ? [] : ['--launch']),
    ...(values['native-session'] === undefined ? [] : ['--native-session']),
  ]
  if (values['in-pane'] !== true) {
    if (given.length === 0) return { nonce: undefined, native: undefined }
    fail(
      `${given.join(' and ')} ${given.length === 1 ? 'is' : 'are'} the app's own launch ` +
        'evidence, and it means nothing without --in-pane',
    )
    return null
  }
  if (given.length !== 1) {
    fail(
      'a pane is launched with exactly one piece of launch evidence — --launch <nonce> or ' +
        `--native-session <id> — and this one carries ${given.length === 0 ? 'neither' : given.join(' and ')}`,
    )
    return null
  }
  return { nonce: values.launch, native: values['native-session'] }
}

/**
 * The marker we were told to seed has to be the launch the ticket redeemed,
 * or this pane would write somebody else's evidence into its own session.
 */
function launchMatches(nonce, ownership) {
  if (nonce === undefined || nonce === ownership.launch) return true
  fail(
    `this pane was told to seed the launch marker ${JSON.stringify(nonce)}, and its ticket ` +
      'redeemed a different launch — refusing rather than binding the wrong session',
  )
  return false
}

/** The lead's own credential: its tab, its named operations, nothing else. */
function leadRequester(app) {
  return appRequester({ url: app.url, token: app.token, tab: app.tab })
}

/**
 * The app, or a refusal that names it — for the verbs that exist only
 * because it does. `cf say` and `cf read` drive a pane, and a pane is the
 * app's; outside one there is nothing for them to talk to.
 */
function requireApp(verb) {
  const app = appHere()
  if (app === null) {
    fail(
      `cf ${verb} is how you reach a pane, and panes live in ConsensFlow's app — ` +
        'run it from a pane the app opened',
    )
    return null
  }
  return app
}

/**
 * An agent must not spawn agents, ask them anything, or read their panes:
 * its own skill would otherwise invite it to. One sentence, six verbs.
 */
function childRefused() {
  if (env.CONSENSFLOW_CHILD !== '1') return false
  fail('this is already an agent run — an agent does not spawn agents')
  return true
}

/**
 * What a consult became, in one line.
 *
 * `--new` is the only way this side knows a conversation is new: the app
 * answers `opened` both for a name it just minted and for one it reopened,
 * and inventing "(new)" for the first would be a claim nothing here can
 * check. So an unasked-for open says `opened` and stops there.
 */
function consultLine(answer, fresh) {
  if (answer.outcome === 'unknown') {
    return [
      `conversation: ${answer.conversation} (unknown) — launch ${answer.launch}`,
      '  the pane host never came back: the launch is unresolved, and nothing may',
      `  launch ${answer.conversation} again until its pane ends`,
    ].join('\n')
  }
  const state = answer.outcome === 'said' ? 'continuing' : fresh === true ? 'new' : 'opened'
  return `conversation: ${answer.conversation} (${state}) — pane ${answer.pane?.id}`
}

async function runVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: {
      brief: { type: 'string' },
      context: { type: 'string' },
      'prompt-file': { type: 'string' },
      'handoff-file': { type: 'string' },
      'no-handoff': { type: 'boolean', default: false },
      image: { type: 'string', multiple: true },
      json: { type: 'boolean', default: false },
      new: { type: 'boolean', default: false },
      session: { type: 'string' },
      // The app's own launch path. `--in-pane` says "the app opened this
      // pane and started me in it"; the two evidence flags carry what
      // `childEnv` strips out of the environment before a harness runs —
      // the launch nonce for the seed's first line, or the native session
      // id the harness must take.
      'in-pane': { type: 'boolean', default: false },
      launch: { type: 'string' },
      'native-session': { type: 'string' },
      // The lead's standing answer to "deliver this conversation's replies
      // to me or wait until I ask" — the one preference the precedence
      // table takes from a lead, and only the app can honour it.
      notify: { type: 'string' },
    },
  })

  if (childRefused()) return

  const name = String(positionals[0] ?? '').replace(/^@/, '')
  const row = name.length > 0 ? agentRow(name, env) : undefined
  if (row === undefined) {
    const known = listAgents(env).map((a) => a.name)
    fail(
      known.length === 0
        ? 'no agents yet — add one with `cf agent add <name>` or in the app'
        : `no agent named ${JSON.stringify(name)}; you have: ${known.join(', ')}`,
    )
    return
  }

  // Two sources for one field is a contradiction, and the old `?:` resolved it
  // silently and wrongly: `--prompt-file` REPLACED the quoted task, so a lead
  // that passed both had its own sentence thrown away without a word. Live,
  // 2026-09-02: the quoted task named the files to read first and the order to
  // work in, and none of it ever reached the agent. Short framing beside a
  // long body is what `--brief` and `--context` are for.
  const quoted = positionals.slice(1).join(' ')
  if (values['prompt-file'] !== undefined && quoted.trim().length > 0) {
    fail(
      'a task in a file and a task in quotes are two tasks, and --prompt-file replaces the quoted one. ' +
        'Keep the file, and put the framing in --brief "<why>" or --context "<note>".',
    )
    return
  }
  const task =
    values['prompt-file'] !== undefined ? readFileSync(values['prompt-file'], 'utf8') : quoted
  if (task.trim().length === 0) {
    fail('give the agent something to do: cf run @name "<task>" (or --prompt-file <file>)')
    return
  }

  // Handoff context is explicit: the lead passes a file to the worker.
  const handoff =
    values['no-handoff'] || values['handoff-file'] === undefined
      ? ''
      : readFileSync(values['handoff-file'], 'utf8')

  const cwd = process.cwd()

  // An image agent draws and stops: there is no conversation to continue and
  // no window to open, so it never joins the threading below. Say so rather
  // than accepting a flag and ignoring it — a lead asked for `--session` here
  // and then hunted for a conversation that was never going to exist (live,
  // 2026-08-24).
  // The app's own flags, checked before anything acts on them. They used to
  // be recognised and then fall through — `--launch` without `--in-pane` ran
  // an ordinary cmux consult, and an image agent (whose branch comes first)
  // ignored all four in silence. A flag we do not honour is refused.
  const standalone = appHere()
  if (row.kind === 'image') {
    for (const flag of ['in-pane', 'launch', 'native-session', 'notify']) {
      if (values[flag] !== undefined && values[flag] !== false) {
        fail(
          `@${row.id} draws images; it holds no conversation and no pane, so --${flag} means nothing here`,
        )
        return
      }
    }
  }
  if (launchEvidenceOf(values) === null) return
  if (values.notify !== undefined && values['in-pane']) {
    fail(
      '--notify is the lead’s standing preference for a conversation, recorded when the ' +
        'consult is made; a pane cannot set it for the lead that opened it',
    )
    return
  }
  if (values.notify !== undefined && standalone === null) {
    fail(
      '--notify records a delivery preference, and delivering an answer is the app’s job: ' +
        'run it from a pane ConsensFlow opened',
    )
    return
  }
  if (!values['in-pane'] && standalone === null) {
    requireApp('run')
    return
  }
  if (row.kind === 'image') {
    for (const flag of ['session', 'new']) {
      if (values[flag] !== undefined && values[flag] !== false) {
        fail(`@${row.id} draws images; it holds no conversation, so --${flag} means nothing here`)
        return
      }
    }
    const result = await runImageAgent({
      cwd,
      agent: row,
      prompt: task,
      imagePaths: values.image ?? [],
    })
    out(values.json ? JSON.stringify(result, null, 2) : renderImageRun(result))
    // A failed generation must fail the command, or a caller that only checks
    // the exit code believes it has a picture.
    if (!result.ok) process.exitCode = 1
    return
  }

  // The app owns launches; this process is either the requester or its controller.
  if (values['in-pane']) {
    await runInPane(row, task, values, handoff)
    return
  }
  if (standalone !== null) {
    await runThroughApp(standalone, row, task, values)
    return
  }

  requireApp('run')
}

/**
 * Spend this pane's launch ticket for what it authorises.
 *
 * Ownership — which conversation this pane is, in which tab, at which
 * generation — comes back from the app, and so does a capability scoped to
 * this one launch. Nothing here is taken from the environment beyond the
 * ticket itself, and nothing from argv: a pane that could name its own
 * conversation could write to somebody else's.
 */
async function redeemLaunch() {
  const app = appHere()
  if (app === null || typeof app.ticket !== 'string' || app.ticket.length === 0) {
    fail(
      "--in-pane is the app's own launch path: it runs on a single-use launch ticket, " +
        "and only ConsensFlow's app issues one",
    )
    return null
  }
  const ownership = await appRequester({ url: app.url })
    .redeem(app.ticket)
    .catch((cause) => {
      // The app answers a spent, expired or revoked ticket with a bare
      // `unauthorized`, which tells a pane nothing about what to do next.
      if (cause?.status !== 401) throw cause
      throw new Error(
        'this launch ticket is spent, expired or revoked — a pane is launched once, ' +
          "and only ConsensFlow's app relaunches it",
      )
    })
  return {
    app,
    ownership,
    controller: appRequester({
      url: app.url,
      token: ownership.capability,
      launch: ownership.launch,
      generation: ownership.generation,
    }),
  }
}

/**
 * A consult in the app: one POST, one line back, and no window here.
 *
 * The lead's pane stays the lead's. The app applies the continuation rule
 * against its own records — it knows this lead's conversations, we do not —
 * so nothing is resolved on this side and no row is written on this side.
 */
async function runThroughApp(app, row, task, values) {
  // `--new --session <name>` is the cmux idiom: mint a name, then create it.
  // Here the app mints — it holds the records the name has to be unique
  // against — so the two flags contradict each other, and the app's own
  // refusal ("two different asks") leaves a lead that learnt that idiom with
  // nowhere to go. Both ways out, named, and the conversation with them.
  if (values.new === true && values.session !== undefined) {
    fail(
      'in ConsensFlow --new mints the conversation name itself and prints it, so it cannot be ' +
        `given one too: drop --session and --new mints one, or drop --new to continue ${JSON.stringify(values.session)}.`,
    )
    return
  }
  const answer = await leadRequester(app).post('consult', {
    opId: randomUUID(),
    agent: row.id,
    task,
    ...(values.new === true ? { fresh: true } : {}),
    ...(values.session === undefined ? {} : { session: values.session }),
    ...(typeof values.brief === 'string' ? { brief: values.brief } : {}),
    ...(typeof values.context === 'string' ? { context: values.context } : {}),
    ...(values['no-handoff'] || values['handoff-file'] === undefined
      ? {}
      : { handoffFile: values['handoff-file'] }),
    ...(values.notify === undefined ? {} : { notify: values.notify }),
  })
  out(values.json ? JSON.stringify(answer, null, 2) : consultLine(answer, values.new === true))
}

/**
 * The process the app started IN the pane it opened.
 *
 * Everything this writes goes through the launch capability redemption
 * hands back, and the conversation it writes to comes from that redemption
 * too — never from `--session`, never from `leadId(env)`. A pane's process
 * is trusted with exactly one launch, and the ticket says which.
 */
async function runInPane(row, task, values, handoff) {
  const launch = await redeemLaunch()
  if (launch === null) return
  const { app, ownership, controller } = launch
  const name = ownership.conversation
  const nonce = values.launch
  if (!launchMatches(nonce, ownership)) return
  // `--native-session` names the session this pane must BE. On a fresh
  // launch (`--new`) the app preallocated it; otherwise the conversation
  // already holds it and this is a reopening, which is the difference
  // between `claude --session-id` and `claude --resume`.
  const native = values['native-session']
  const resuming = native !== undefined && values.new !== true
  // What this launch puts in the pane. Every harness but one gets a window
  // seed; kimi takes its prompt in argv and opens no window, so its first
  // turn IS a packet — and the launch marker has to ride on the first line
  // of whichever of the two the harness actually receives, because that is
  // the text its own store keeps and `bindEvidence` reads back.
  const seed =
    row.kind === 'kimi'
      ? await createPacket({
          cwd: cwdOf(),
          agent: row,
          kind: 'ask',
          task,
          brief: values.brief,
          extraContext: values.context,
          handoff,
          // A reopened conversation already has the workspace, the how-to-work
          // and the scene: re-sending them buries the actual question.
          continuing: resuming,
          conversational: true,
          nonce: nonce ?? null,
        })
      : createWindowSeed({
          task,
          brief: values.brief,
          extraContext: values.context,
          handoff,
          nonce: nonce ?? null,
        })
  // Recorded before it goes in: a seed that reaches the harness and no
  // record of it is a turn the lead cannot account for.
  await controller.post('sent.record', {
    opId: randomUUID(),
    entry: { kind: 'seed', chars: seed.length, pane: app.pane },
  })

  // A session the app named is known before the window is, so it binds
  // first — the store decides on what evidence (the id it preallocated, or
  // the id it already had reported), from the launch record it holds. A
  // refusal is carried, not thrown: the window still opens.
  const preBind = native === undefined ? null : await bindSession(controller, { sessionId: native })

  // kimi can neither be handed a task nor typed into, so its turn streams
  // here — on the session it is resuming, when there is one — and only then
  // does the pane become its window.
  if (row.kind === 'kimi') {
    await streamKimiTurn(controller, row, name, seed, { native, nonce, preBind })
    return
  }

  if (native !== undefined) {
    const invocation = resuming
      ? interactiveResume(row, native, seed)
      : interactiveStart(row, native, seed)
    if (invocation === null) {
      fail(
        resuming
          ? `${row.kind} has no way to reopen ${native}`
          : `${row.kind} cannot open a window on a session it was given`,
      )
      return
    }
    await handOver(name, row.id, await withDeliveryChannel(invocation, row.kind, ownership.launch))
    reportBinding(preBind)
    return
  }

  // Everything left is a fresh codex or opencode: they mint their own id and
  // tell only their own store, so the window and the search for it run
  // together — a person may sit on codex's trust prompt for half an hour
  // before there is a session at all.
  const discover = DISCOVER[row.kind]
  if (discover === undefined) {
    fail(`${row.kind} cannot open a window here`)
    return
  }
  await openAndDiscover(controller, row, name, seed, discover, nonce)
}

/**
 * kimi's turn, streamed, and then its window.
 *
 * `-p` is defined as non-interactive and there is no way to seed an
 * interactive kimi, so the task is streamed on the session (resumed with
 * `-S` when the app named one) and the pane becomes `kimi -S <id>`
 * afterwards. Without that handover a kimi pane would print an answer and
 * die, which is the one shape every other harness avoids.
 */
async function streamKimiTurn(controller, row, name, seed, { native, nonce, preBind }) {
  await controller.post('progress.set', { progress: { state: 'first-turn' } })
  const started = Date.now() - 2000
  const result = await runAgent({
    cwd: cwdOf(),
    agent: row,
    packet: seed,
    kind: 'ask',
    onEvent: (event) => {
      const line = renderEvent(event)
      if (line) out(line)
    },
    session: native === undefined ? {} : { sessionId: native },
  })
  await controller.post('progress.set', {
    progress: { state: 'first-turn-done', exitCode: result.exitCode ?? null },
  })
  const sessionId = native ?? result.sessionId ?? (await discoverKimiSession(cwdOf(), started, env))
  if (!sessionId) {
    // No session means no window to hand over and nothing for the app to
    // read: the pane is empty and the lead is owed the reason, not a
    // silent success.
    fail(
      `${row.kind} captured no session on its first turn (it exited ` +
        `${result.exitCode ?? 'without a status'}), so this pane has no window and the ` +
        'app has nothing to read',
    )
    return
  }
  const bound =
    native === undefined ? await bindDiscovered(controller, row.kind, sessionId, nonce) : preBind
  const invocation = interactiveResume(row, sessionId)
  if (invocation !== null) await handOver(name, row.id, invocation)
  reportBinding(bound)
}

/** A cold window, and the search for the session it mints, run together. */
async function openAndDiscover(controller, row, name, seed, discover, nonce) {
  const invocation = interactiveStart(row, null, seed)
  if (invocation === null) {
    fail(`${row.kind} cannot open a window here`)
    return
  }
  const since = Date.now() - 2000
  let windowUp = true
  let closingAt = Number.POSITIVE_INFINITY
  const searching = () => windowUp || Date.now() < closingAt
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
  const found = (async () => {
    let wait = 500
    while (searching()) {
      const id = await discover(cwdOf(), since, env, { seed })
      if (id !== null) return await bindDiscovered(controller, row.kind, id, nonce)
      const until = Date.now() + wait
      while (searching() && Date.now() < until) await sleep(250)
      wait = Math.min(wait * 2, 5000)
    }
    // One last exact look — a store written on the way out still counts.
    // Only the seeded match: cmux's fallback guess ("the earliest session
    // that appeared here") is a guess, and a guess is not launch evidence.
    const id = await discover(cwdOf(), since, env, { seed })
    if (id !== null) return await bindDiscovered(controller, row.kind, id, nonce)
    return { bound: false, reason: `no ${row.kind} session appeared for this launch` }
  })()
  // The window decides the exit status, once, and the search never touches
  // it: the same launch used to end 0 or 1 depending on whether discovery
  // finished before the window did.
  await handOver(name, row.id, invocation)
  windowUp = false
  closingAt = Date.now() + 3000
  const outcome = await found
  if (outcome?.bound !== true) warn(outcome?.reason ?? 'this launch stays unbound')
}

/**
 * Bind the session discovery found, on the turn that carries our nonce.
 *
 * The turn is read back from the harness's own store rather than assumed
 * from the seed we sent: discovery matches a session that CONTAINS the seed,
 * and `bindEvidence` asks a stricter question — is the marker the opening
 * line of that turn. Read raw, through the completion adapters: the display
 * reader strips the marker before anyone sees it, which is exactly the text
 * the evidence lives in.
 */
/**
 * Ask the app to bind a session, and answer instead of throwing.
 *
 * A controller op can be refused — the conversation moved, the launch is
 * over — and that refusal used to travel as an exception: out of an
 * unawaited discovery promise as a raw stack, or out of the kimi path
 * before the window was ever handed over. It is a verdict like any other.
 */
async function bindSession(controller, candidate) {
  try {
    await controller.post('session.bind', { candidate })
    return { bound: true }
  } catch (cause) {
    return {
      bound: false,
      reason:
        `the app refused to bind ${candidate.sessionId}: ` +
        `${cause instanceof Error ? cause.message : String(cause)}`,
    }
  }
}

/**
 * The delivery channel a worker's harness has to carry on its own argv.
 *
 * A pi worker records settlement evidence through its extension, and a Pi
 * conversation without it is not eligible for automatic delivery — so the
 * extension is not optional for a pane the app launched. `src/channels.js`
 * owns what that costs (the flag, and the inbox/ack paths in the
 * environment); this only puts it in front of the arguments the harness
 * already had, so a positional prompt stays last.
 */
async function withDeliveryChannel(invocation, kind, launchId) {
  if (kind !== 'pi') return invocation
  const configured = await launchConfiguration(kind, { launchId, workspace: cwdOf() })
  return {
    ...invocation,
    args: [...configured.args, ...invocation.args],
    env: { ...invocation.env, ...configured.env },
  }
}

/** A binding verdict reported where it cannot become the command's answer. */
function reportBinding(result) {
  if (result !== null && result !== undefined && result.bound !== true) warn(result.reason)
}

async function bindDiscovered(controller, kind, sessionId, nonce) {
  const read = await harnessAnswers(kind, sessionId, env)
  if (!Array.isArray(read.items)) {
    // Nothing was read, which is a different thing from nothing matching:
    // say which, in the adapter's own words, or a supported-version gap
    // reads as an agent that never carried our marker.
    return {
      bound: false,
      reason:
        `${kind} session ${sessionId} could not be read, so it stays unbound: ` +
        `${read.reason ?? 'no reason given'}`,
    }
  }
  const turn = read.items
    .filter((item) => item.role === 'user')
    .slice(0, TURNS_EXAMINED)
    .find((item) => openingLineCarriesNonce(item.text, nonce))
  if (turn === undefined) {
    const opening = read.items.find((item) => item.role === 'user')?.text?.split('\n')[0]
    return {
      bound: false,
      reason:
        `${kind} session ${sessionId} carries no launch marker for this launch — ` +
        `it stays unbound rather than bound on a guess (its first user turn opens ${JSON.stringify(opening ?? '')})`,
    }
  }
  return await bindSession(controller, { sessionId, turn: turn.text })
}

/** Words into a live pane, through the app that owns it. */
async function sayVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { json: { type: 'boolean', default: false } },
  })
  if (childRefused()) return
  const app = requireApp('say')
  if (app === null) return
  const session = String(positionals[0] ?? '')
  const text = positionals.slice(1).join(' ')
  if (session.length === 0 || text.trim().length === 0) {
    fail('say which conversation and what: cf say <conversation> "<words>"')
    return
  }
  const answer = await leadRequester(app).post('say', { opId: randomUUID(), session, text })
  out(
    values.json
      ? JSON.stringify(answer, null, 2)
      : `said: ${answer.conversation} — pane ${answer.pane?.id}`,
  )
}

/**
 * A part number as the app counts them: a whole number from 1.
 *
 * `parseInt` reads "2garbage" as 2 and would print a different part than
 * the one asked for, which is the one thing a receipt cannot survive.
 * Answers null once it has said why it refuses, so the verbs share one
 * check and no refused number ever reaches the app.
 */
function parsePartNumber(asked) {
  const part = Number(asked)
  if (!/^[0-9]+$/.test(asked) || !Number.isSafeInteger(part) || part < 1) {
    fail(`a part is a whole number from 1, and ${JSON.stringify(asked)} is not one`)
    return null
  }
  return part
}

/**
 * What is left to read, said on stderr — stdout stays the part verbatim.
 *
 * The app answers a result read with the immutable delivery id that owns the
 * parts, so every follow-up part goes through it: `cf read d-N --part k`.
 * Only when the answer carries no delivery id does the teaching fall back to
 * the conversation form that started the read.
 */
function teachRemainingParts(session, answerId, answer) {
  const index = answer.index ?? answer.k
  const total = answer.total ?? answer.of
  if (!Number.isSafeInteger(index) || !Number.isSafeInteger(total)) return
  if (index >= total) return
  const next = index + 1
  const where =
    typeof answer.deliveryId === 'string' && answer.deliveryId.length > 0
      ? `cf read ${answer.deliveryId} --part ${next}`
      : `cf read ${session}${answerId === undefined ? '' : ` --answer ${answerId}`} --part ${next}`
  warn(`part ${index} of ${total} — next: ${where}`)
}

/**
 * One complete framed part of a result, printed exactly as the app framed it.
 *
 * Two forms: `cf read d-N` reads on with a delivery the app already minted
 * (its first read returned the id), while `cf read <conversation>` starts
 * from the conversation — the app selects its oldest unread completed
 * result, or the one `--answer` names. Either way stdout is the part
 * verbatim to the byte, next-part line included: the framing IS the receipt
 * the app matches against the lead's tool result, so a newline this side
 * adds or eats is a delivery that cannot be proved. Reading marks nothing:
 * the native receipt is the only authority for "read", and a printed
 * attempt never moves it.
 */
async function readVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { answer: { type: 'string' }, part: { type: 'string' } },
  })
  if (childRefused()) return
  const app = requireApp('read')
  if (app === null) return
  const target = String(positionals[0] ?? '')
  if (target.length === 0) {
    fail(
      'name what to read: cf read <conversation> [--answer <id>] [--part <k>], ' +
        'or carry on with cf read <delivery id> [--part <k>]',
    )
    return
  }
  const part = parsePartNumber(values.part ?? '1')
  if (part === null) return
  if (/^d-[0-9]+$/.test(target)) {
    if (values.answer !== undefined) {
      fail(
        `cf read ${target} already names its answer — it takes only --part, not --answer; ` +
          `to select a result, read its conversation instead: cf read <conversation> --answer <id>`,
      )
      return
    }
    const answer = await leadRequester(app).post('read', {
      opId: randomUUID(),
      deliveryId: target,
      part,
    })
    process.stdout.write(String(answer.text ?? ''))
    return
  }
  const answer = await leadRequester(app).post('results.read', {
    opId: randomUUID(),
    session: target,
    ...(values.answer === undefined ? {} : { answerId: values.answer }),
    part,
  })
  process.stdout.write(String(answer.text ?? ''))
  teachRemainingParts(target, values.answer, answer)
}

/**
 * Everything ConsensFlow installed, taken back: both host payloads, every
 * file the manifest owns, the roster, and local run artifacts.
 */
function resetVerb(rest) {
  const { values } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { yes: { type: 'boolean', default: false } },
  })

  // Counting before refusing makes the refusal the preview: the same two
  // numbers the page puts in its dialog, printed while nothing has been
  // touched. `off` can be undone by opening the app again. This is not: a roster is typed by hand, and a packet, a transcript
  // or a generated image exists nowhere else.
  const { agents, runs } = resetPreview(env)
  if (!values.yes) {
    out(`reset would remove ${plural(agents, 'agent')} and ${plural(runs, 'run')} (packets,`)
    out('transcripts, generated images), every file ConsensFlow installed — including skill')
    out("files you have edited yourself, the `cf` launcher, and the desktop app's own")
    out('caches. The ConsensFlow.app bundle itself stays — remove it in Finder if you')
    out('want it gone.')
    out('')
    fail('nothing was touched. Re-run with --yes if that is what you want')
    return
  }

  const outcome = resetEverything(env)
  for (const change of outcome.changes) {
    const what = change.path ?? change.host
    if (what !== undefined) out(`${String(change.action ?? 'removed').padEnd(16)} ${what}`)
  }
  out(
    `ConsensFlow is reset — ${plural(outcome.removed.agents, 'agent')} and ${plural(outcome.removed.runs, 'run')} went with it`,
  )
  out('The ConsensFlow.app bundle is untouched; remove it in Finder if you want it gone.')
}

function plural(count, noun) {
  return `${count} ${noun}${count === 1 ? '' : 's'}`
}

/** Direct people to the conversation controls in the app. */
async function chatVerb() {
  if (childRefused()) return
  const app = appHere()
  fail(
    `Conversations live in ConsensFlow app panes${app === null ? '' : ` at ${app.url}`}. Use cf say <conversation> "<words>" or cf attach <conversation>.`,
  )
}

/** Focus or reopen a conversation through the app that owns it. */
async function attachVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: {
      print: { type: 'boolean', default: false },
      'in-pane': { type: 'boolean', default: false },
      // The app puts the same launch evidence on every `pane.open` argv,
      // reopening included. A resume needs none of it — the session it
      // opens was bound when it was created — but it must be accepted, or
      // the pane the app just opened dies on its own command line.
      launch: { type: 'string' },
      'native-session': { type: 'string' },
    },
  })
  if (childRefused()) return

  const asked = String(positionals[0] ?? '')
  const evidence = launchEvidenceOf(values)
  if (evidence === null) return
  if (values['in-pane']) {
    await attachInPane(evidence)
    return
  }
  const standalone = appHere()
  if (standalone !== null) {
    await attachThroughApp(standalone, asked, values)
    return
  }
  requireApp('attach')
}

/**
 * A window on a conversation, opened by the app rather than by us.
 *
 * The app decides whether that conversation already has a live pane (it
 * answers with it) or needs one (it opens it and runs `cf attach --in-pane`
 * inside it). Either way no window opens in the pane we are standing in.
 */
async function attachThroughApp(app, asked, values) {
  if (values.print) {
    fail('--print is unavailable: ConsensFlow opens the pane; use cf attach <conversation>')
    return
  }
  // `@agent` and a bare `cf attach` name no conversation, and only the
  // records can turn them into one. Read, never written, from this side.
  let session = asked
  if (asked.length === 0 || asked.startsWith('@')) {
    const threads = await loadThreads(cwdOf())
    const picked = pickConversation(threads, asked)
    if (picked.record === undefined) {
      fail(noConversationHere(asked, Object.keys(threads), env))
      return
    }
    session = picked.name
  }
  const answer = await leadRequester(app).post('attach', { opId: randomUUID(), session })
  out(
    answer.outcome === 'live'
      ? `${answer.conversation} is already open — pane ${answer.pane?.id}`
      : `${answer.conversation} — pane ${answer.pane?.id}`,
  )
}

/**
 * The reopening side of that: this process IS the new pane.
 *
 * A resume needs no fresh launch evidence — the session it opens is the one
 * already bound to this conversation, proved when it was created. So this
 * redeems for its ownership, reads the session id the app recorded, and
 * hands the pane to the harness.
 */
async function attachInPane({ nonce, native }) {
  const launch = await redeemLaunch()
  if (launch === null) return
  if (!launchMatches(nonce, launch.ownership)) return
  const name = launch.ownership.conversation
  const record = (await loadThreads(cwdOf()))[name]
  if (record === undefined) {
    fail(`the app opened a pane for ${name}, which is not a conversation here`)
    return
  }
  // The session comes from the app, on the command line, like every other
  // launch: it is the app that decided this pane reopens that session, and
  // re-reading the row here would answer a question already answered.
  if (typeof native !== 'string' || native.length === 0) {
    fail(`${name} was reopened without a session to reopen — the app names it`)
    return
  }
  const row = agentRow(record.agent, env)
  const invocation = interactiveResume(row ?? { kind: record.kind }, native)
  if (invocation === null) {
    fail(`${record.kind} has no way to reopen ${native}`)
    return
  }
  await handOver(
    name,
    record.agent,
    await withDeliveryChannel(invocation, record.kind, launch.ownership.launch),
  )
}

/**
 * Every completed worker result in this tab, with its status and preview.
 *
 * Discovery only: the listing comes from the app, which holds the tab's
 * conversations, their completions and their delivery records — nothing is
 * read from this workspace's files, and nothing is marked seen. An exact
 * conversation name narrows to that conversation; `@agent` narrows to that
 * roster agent's conversations, the same `@`-means-agent rule every verb
 * shares. Reading one whole is `cf read`'s job.
 */
async function resultsVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { json: { type: 'boolean', default: false } },
  })
  if (childRefused()) return
  const app = requireApp('results')
  if (app === null) return
  if (positionals.length > 1) {
    fail('cf results takes at most one filter: a conversation or @agent')
    return
  }
  const asked = String(positionals[0] ?? '')
  const answer = await leadRequester(app).post('results.list', { opId: randomUUID() })
  const workers = Array.isArray(answer.workers) ? answer.workers : []
  const kept =
    asked.length === 0
      ? workers
      : asked.startsWith('@')
        ? workers.filter((worker) => worker.agent === asked.slice(1))
        : workers.filter((worker) => worker.conversation === asked)
  if (asked.length > 0 && kept.length === 0) {
    fail(
      noConversationHere(
        asked,
        workers.map((worker) => worker.conversation),
        env,
      ),
    )
    return
  }
  if (values.json) {
    out(JSON.stringify({ workers: kept }, null, 2))
    return
  }
  if (kept.length === 0) {
    out('no worker conversations here yet — `cf run @name "<task>"` starts one')
    return
  }
  for (const worker of kept) {
    const results = Array.isArray(worker.results) ? worker.results : []
    const state = worker.running === true ? 'running' : 'idle'
    const reason =
      typeof worker.reason === 'string' && worker.reason.length > 0 ? ` (${worker.reason})` : ''
    out(
      `${worker.conversation} · @${worker.agent} — ${state}${reason}: ` +
        `${results.length} completed result${results.length === 1 ? '' : 's'}`,
    )
    for (const result of results) {
      const onwards =
        typeof result.deliveryId === 'string' && result.deliveryId.length > 0
          ? ` — cf read ${result.deliveryId}`
          : ''
      out(`  ${result.id} ${result.status} ${result.bytes}B — ${result.preview}${onwards}`)
    }
  }
}

/**
 * `cf catchup` is retired: completed results are discovered with
 * `cf results` and read whole with `cf read`. The name stays a verb so the
 * error names the replacements instead of reading as an unknown command —
 * and it never touches the app, the transcript, or any read mark.
 */
async function catchupVerb() {
  if (childRefused()) return
  fail(
    'cf catchup is retired — discover completed results with `cf results [conversation|@agent]`, ' +
      'then read one whole with `cf read <conversation> [--answer <id>] [--part <k>]`',
  )
}

/**
 * Replace this terminal with the harness's window and exit with its code.
 *
 * The window is the same agent with a screen, so it gets the same environment
 * guards a run does — billing keys stripped, cmux control stripped, the child
 * marker set. For a while this spawned with the full inherited environment,
 * which meant every attached turn could silently bill an API key.
 */
async function handOver(name, agent, invocation) {
  out(`${name} · @${agent} — handing this terminal to ${invocation.command}`)
  const child = spawn(invocation.command, invocation.args, {
    cwd: cwdOf(),
    stdio: 'inherit',
    env: childEnv(process.env, invocation),
  })
  // A binary that is not on PATH emits `error`, never `close`, and an
  // unhandled `error` on a child process is an uncaught exception: a pane
  // whose harness is missing printed Node's stack instead of a sentence.
  const outcome = await new Promise((resolve) => {
    child.once('error', (cause) => resolve({ cause }))
    child.once('close', (code) => resolve({ code }))
  })
  if (outcome.cause !== undefined) {
    fail(
      `${invocation.command} could not be started: ${outcome.cause.message} — ` +
        `is ${invocation.command} installed and on this pane's PATH?`,
    )
    return
  }
  process.exitCode = outcome.code ?? 0
}

async function sessionsVerb(rest) {
  const { values } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { json: { type: 'boolean', default: false } },
  })
  const cwd = process.cwd()
  const threads = await loadThreads(cwd)
  if (values.json) {
    out(JSON.stringify(threads, null, 2))
    return
  }
  const names = Object.keys(threads)
  if (names.length === 0) {
    out('no conversations here yet — `cf run @name "<task>"` starts one')
    return
  }
  for (const name of names.sort()) {
    const row = threads[name]
    const runs = `${row.runs} run${row.runs === 1 ? '' : 's'}`
    // A row still carrying `startedAt` is one whose run has not come back.
    const when = row.startedAt ? `working since ${row.startedAt}` : (row.lastRunAt ?? '')
    out(`${name.padEnd(18)}@${String(row.agent).padEnd(12)}${runs.padEnd(9)}${when}`)
  }
}

async function lastVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { json: { type: 'boolean', default: false } },
  })
  const cwd = process.cwd()
  const wanted = String(positionals[0] ?? '')
  const threads = await loadThreads(cwd)
  const names = Object.keys(threads)

  // A conversation name, an @agent (that agent's most recent conversation
  // here), or nothing — the newest one, same as bare `cf attach`.
  const { name, record: row } = pickConversation(threads, wanted)
  if (row === undefined) {
    fail(noConversationHere(wanted, names, env))
    return
  }
  // A conversation whose turns all happened in the agent's own window has no
  // runs of ours to read — the harness's session store is the record.
  if (row.lastRunId == null) {
    if (values.json) {
      out(JSON.stringify({ session: name, agent: row.agent, window: true }, null, 2))
      return
    }
    out(`${name} · @${row.agent} — its turns live in the agent's own window`)
    out(`read them whole with: cf read ${name}`)
    return
  }
  const runDir = join(runsRoot(cwd), String(row.lastRunId))
  const result = readJsonFile(join(runDir, 'result.json'))
  if (result === undefined) {
    fail(`the run for ${name} left no result.json at ${runDir}`)
    return
  }
  if (values.json) {
    // Spread first: result.json carries the whole agent row under `agent`,
    // and the caller asked for the conversation's agent NAME.
    out(JSON.stringify({ ...result, session: name, agent: row.agent }, null, 2))
    return
  }
  out(`# ${name} · @${row.agent}`)
  out('')
  out(boundedAnswer(String(result.output ?? '').trim() || '(no answer)'))
  out('')
  out(`transcript: ${join(runDir, 'transcript.md')}`)
}

/**
 * A backstop, not a substitute for extracting the answer properly.
 *
 * `result.output` is whatever the run recorded, and a harness whose stream we
 * cannot parse records ALL of it: a kimi run measured 493,390 characters, and
 * `cf last` pasted every one of them into the lead that asked (2026-08-26 —
 * kimi has its own extractor now). The fix for that is upstream, in
 * `findFinalJsonOutput`; this is the wall that stops the NEXT harness doing it
 * before anyone notices. `--json` is untouched: a program asked for the record
 * and can hold it.
 */
const ANSWER_LIMIT = 8000
function boundedAnswer(answer) {
  if (answer.length <= ANSWER_LIMIT) return answer
  const rest = answer.length - ANSWER_LIMIT
  return `${answer.slice(0, ANSWER_LIMIT)}\n\n[…${rest} more characters — this run's answer was never extracted; the whole record is in transcript.md and result.json]`
}

function readJsonFile(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    return undefined
  }
}

function offVerb(rest) {
  const { values } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { force: { type: 'boolean', default: false } },
  })
  const outcome = turnOff(env, { force: values.force })
  for (const change of outcome.changes) {
    const what = change.path ?? `the ${change.host} integration`
    out(`${String(change.action ?? 'removed').padEnd(16)} ${what}`)
  }
  out('ConsensFlow is off — agents are kept in ~/.consensflow/agents.json')
}

function catalogVerb(rest) {
  const { values } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { harness: { type: 'string' }, json: { type: 'boolean', default: false } },
  })

  const catalog =
    values.harness === undefined ? CATALOG : { [values.harness]: CATALOG[values.harness] ?? [] }

  if (values.json) {
    out(JSON.stringify({ catalog }, null, 2))
    return
  }
  for (const [harness, entries] of Object.entries(catalog)) {
    out(`${harness}:`)
    // Width from the rows, not a guess: the OpenCode Go and Zen ids added on
    // 2026-09-06 run to 40 characters and ran straight into the effort column.
    const modelWidth = Math.max(34, ...entries.map((entry) => entry.model.length + 2))
    for (const entry of entries) {
      out(
        `  ${entry.name.padEnd(12)}${entry.model.padEnd(modelWidth)}${(entry.effort ?? '-').padEnd(8)}${entry.description}`,
      )
    }
    out('')
  }
  out('add one with `cf agent add <name>` — no other flags needed')
}

function agentVerb(rest) {
  const action = rest[0]
  const { values, positionals } = parseArgs({
    args: rest.slice(1),
    allowPositionals: true,
    options: {
      harness: { type: 'string' },
      model: { type: 'string' },
      effort: { type: 'string' },
      description: { type: 'string' },
      from: { type: 'string' },
      presets: { type: 'string' },
      json: { type: 'boolean', default: false },
      'dry-run': { type: 'boolean', default: false },
    },
  })
  const name = positionals[0]

  switch (action) {
    case 'add': {
      const added = addAgent(resolveAdd(name, values), env)
      refreshSkill(env)
      out(`${added.name}  ${added.harness}  ${added.model}`)
      return
    }
    case 'list': {
      const agents = listAgents(env)
      if (values.json) {
        out(JSON.stringify({ agents }, null, 2))
        return
      }
      if (agents.length === 0) {
        out('no agents yet — add one with `cf ui` or `cf agent add`')
        return
      }
      for (const p of agents) {
        out(`${p.name.padEnd(14)}${p.harness.padEnd(10)}${p.model.padEnd(36)}${p.effort ?? '-'}`)
      }
      return
    }
    case 'edit': {
      const edited = editAgent(
        name,
        {
          ...(values.model !== undefined ? { model: values.model } : {}),
          ...(values.effort !== undefined ? { effort: values.effort } : {}),
          ...(values.description !== undefined ? { description: values.description } : {}),
        },
        env,
      )
      refreshSkill(env)
      out(`${edited.name}  ${edited.harness}  ${edited.model}`)
      return
    }
    case 'remove': {
      removeAgent(name, env)
      refreshSkill(env)
      out(`removed ${name}`)
      return
    }
    case 'sync': {
      // Catalog-backed agents keep whatever model they were created
      // with; this is how a moved preset reaches them — the description
      // included, so the skill table never names a model the agent dropped.
      // Anything you defined yourself (an explicit --model or --effort at add
      // time records no preset) is left alone.
      const applied = syncAgents(env, { name, dryRun: values['dry-run'] })
      if (applied.length === 0) {
        const backed = listAgents(env).filter((p) => p.preset !== undefined).length
        out(
          backed === 0
            ? 'nothing to sync: no agent came from the catalog'
            : `up to date: all ${backed} catalog-backed agents match the catalog`,
        )
        return
      }
      for (const { name: who, changes } of applied) {
        for (const change of changes) {
          out(
            `${who.padEnd(14)}${change.field.padEnd(14)}${change.from ?? '-'} → ${change.to ?? '-'}`,
          )
        }
      }
      if (values['dry-run']) out('(dry run: nothing was written)')
      else refreshSkill(env)
      return
    }
    default:
      fail('usage: cf agent add|list|edit|remove')
  }
}

function skillsVerb(rest) {
  const action = rest[0]
  const { values } = parseArgs({
    args: rest.slice(1),
    allowPositionals: true,
    options: {
      all: { type: 'boolean', default: false },
      force: { type: 'boolean', default: false },
    },
  })

  switch (action) {
    case 'install': {
      // Every detected harness receives the same generated skill.
      const agents = listAgents(env)
      if (agents.length === 0) {
        fail('the roster is empty — add an agent with `cf ui` or `cf agent add` first')
        return
      }
      printReport(
        installSkill(
          {
            relPath: 'consensflow/SKILL.md',
            content: generateSkill(agents),
            source: 'consensflow',
          },
          env,
          { targets: skillTargets(env, { all: values.all }) },
        ),
      )
      reportNativeHosts(env, values.all)
      // Retire files owned by the old cmux-skills installer.
      syncCmux(env, values)
      return
    }
    case 'update': {
      refreshSkill(env)
      syncCmux(env, values)
      out('updated')
      return
    }
    case 'status': {
      const rows = skillsStatus(env)
      if (rows.length === 0) {
        out('no skills installed')
        return
      }
      const behind = new Set(staleSkills(env))
      for (const row of rows) {
        // `ok` used to mean two things at once: ours and unedited — and
        // current. The first two survive an app upgrade; the third does not.
        const state = behind.has(row.path) ? 'behind' : row.state
        out(`${state.padEnd(9)} ${row.source.padEnd(20)} ${row.path}`)
      }
      if (behind.size > 0) {
        out('')
        out(
          `${behind.size} file${behind.size === 1 ? '' : 's'} carry an older ConsensFlow's text — refresh with: cf skills install`,
        )
      }
      return
    }
    case 'uninstall':
      printReport(uninstallSkills(env, { force: values.force }))
      return
    default:
      fail('usage: cf skills install|update|status|uninstall')
  }
}

function setup(rest) {
  const { values } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: {
      all: { type: 'boolean', default: false },
      force: { type: 'boolean', default: false },
    },
  })

  const installed = installEverywhere(env, values)
  for (const line of installed.report) out(line)
  const harnesses = detectHarnesses(env)
  out(
    harnesses.length > 0
      ? `harnesses: ${harnesses.map((a) => a.id).join(', ')}`
      : 'harnesses: none found on PATH — install claude, codex, pi or opencode and rerun',
  )

  const agents = listAgents(env)
  if (agents.length === 0) {
    // Agents are the user's to create; nothing is seeded for them.
    out(
      'agents: none yet — create them with `cf ui` (or `cf agent add`); the skill installs itself on the first one',
    )
  } else if (harnesses.length > 0) {
    printReport(
      installSkill(
        {
          relPath: 'consensflow/SKILL.md',
          content: generateSkill(agents),
          source: 'consensflow',
        },
        env,
        { targets: skillTargets(env, { all: values.all }) },
      ),
    )
    reportNativeHosts(env, values.all)
  }

  if (harnesses.length > 0) syncCmux(env, values)
}

function doctor() {
  const harnesses = detectHarnesses(env)
  out(`consensflow ${PKG.version}`)
  out(`home:         ${configRoot(env)}`)
  out(
    `harnesses:    ${harnesses.length > 0 ? harnesses.map((a) => `${a.id}${a.native ? ' (has its own consensflow)' : ''}`).join(', ') : 'none on PATH'}`,
  )
  if (existsSync(join(configRoot(env), 'mode.json'))) {
    out('legacy:       mode.json is ignored and can be removed')
  }
  out(`agents:       ${listAgents(env).length}`)
  // Files, not skills — a skill is a directory, and cmux-browser alone is
  // eleven files. Say both, and say whose they are.
  const skills = skillsSummary(env)
  const parts = [`${skills.files} files`]
  if (skills.files > 0) {
    parts.push(
      `${skills.ours} ours` +
        (skills.cmux > 0 ? `, ${skills.cmux} from cmux@${skills.cmuxCommit}` : ''),
    )
    parts.push(
      `${skills.perHarness} skill${skills.perHarness === 1 ? '' : 's'} in each of ${skills.harnesses} harness${skills.harnesses === 1 ? '' : 'es'}`,
    )
  }
  const bad = skills.drifted + skills.missing
  if (bad > 0) parts.push(`${bad} drifted/missing`)
  // An upgrade brings a new skill template; nothing rewrites the installed
  // files until the roster moves, so say it here rather than let a lead read
  // the previous version's prose.
  const behind = staleSkills(env).length
  if (behind > 0) parts.push(`${behind} behind this version`)
  out(`skills:       ${parts.join(' · ')}`)

  // The install records the runtime that performed it — from the app, its own
  // bundled Node. If that has moved, the wiring it left behind stops working,
  // and saying so here is cheaper than letting it fail quietly.
  const wiring = terminalRuntime(env)
  if (wiring !== null) {
    // Three states, not two. A runtime that exists but belongs to ANOTHER
    // ConsensFlow looks healthy from every count on this page while every `cf`
    // the skill teaches runs the other one's code — which is what a second
    // install (an app beside a repo build) leaves behind. Run through the
    // launcher this can never fire, because `cf` IS whatever the launcher
    // started; run from a bundle directly, it is the only thing that can say.
    out(
      !wiring.exists
        ? `runtime:      ${wiring.runtime} — MISSING. Reinstall from the app to point the wiring at its runtime.`
        : wiring.mine
          ? `runtime:      ${wiring.runtime}`
          : `runtime:      ${wiring.runtime} — another ConsensFlow. \`cf\` runs that one; \`cf setup\` from this one claims the command.`,
    )
  }

  // A harness in scope with no skill of ours consults nothing, and every other
  // line here would still look healthy. Name it.
  const gaps = skillGaps(env)
  if (gaps.length > 0) {
    out(
      `missing:      ${gaps.join(', ')} ${gaps.length === 1 ? 'is' : 'are'} in scope but carrying no skill — run \`cf skills install\``,
    )
  }

  // Claude Code's settings are not ours to write, so a hook an older version
  // left there is named rather than removed behind the user's back.
  const stale = staleClaudeHooks(env)
  if (stale.events.length > 0) {
    out(
      `hooks:        ${stale.events.join(', ')} in ${stale.path} still reference consensflow — no version answers them; remove those entries`,
    )
  }
}

async function main() {
  const [command, ...rest] = process.argv.slice(2)

  // A machine set up before the roots were merged keeps its state — it just
  // moves into the one directory, once, and silently: `cf --version` and
  // `--json` are machine output, and a relocation the user cannot act on is
  // not news. `cf doctor` says where things live.
  migrateStateRoot(env)

  if (command === undefined || command === 'help' || command === '--help') {
    out(USAGE)
    return
  }
  if (command === '--version' || command === '-v' || command === 'version') {
    out(PKG.version)
    return
  }

  // cc and pi write the shared roster without telling v3; any invocation is
  // an opportunity to notice and regenerate the installed skill. Skills
  // verbs manage installation explicitly, so they are exempt.
  //
  // `run` belongs here most of all: a lead that only ever consults would
  // otherwise read a skill generated from the old roster until some other
  // verb happened to run. The check is one hash compare on the common path.
  if (['agent', 'setup', 'ui', 'doctor', 'run', 'catalog'].includes(command)) {
    healSkillIfStale(env)
  }

  switch (command) {
    case 'use':
    case 'mode':
      fail(
        'ConsensFlow has one shape now: the standalone app. Open ConsensFlow to install its CLI and skill.',
      )
      out(USAGE)
      return
    case 'catchup':
      await catchupVerb()
      return
    case 'results':
      await resultsVerb(rest)
      return
    case 'attach':
      await attachVerb(rest)
      return
    case 'say':
      await sayVerb(rest)
      return
    case 'read':
      await readVerb(rest)
      return
    case 'chat':
      await chatVerb(rest)
      return
    case 'sessions':
      await sessionsVerb(rest)
      return
    case 'mint':
      fail(
        'The app creates conversation names. Use cf run @name "<task>" --new and use the name it prints.',
      )
      return
    case 'last':
      await lastVerb(rest)
      return
    case 'off':
      offVerb(rest)
      return
    case 'reset':
      resetVerb(rest)
      return
    case 'run':
      await runVerb(rest)
      return
    case 'catalog':
      catalogVerb(rest)
      return
    case 'agent':
      agentVerb(rest)
      return
    case 'skills':
      skillsVerb(rest)
      return
    case 'setup':
      setup(rest)
      return
    case 'ui': {
      const { serveUi } = await import('../src/ui.js')
      const { values } = parseArgs({
        args: rest,
        allowPositionals: true,
        options: {
          json: { type: 'boolean', default: false },
          'no-open': { type: 'boolean', default: false },
        },
      })
      await serveUi(env, {
        onOut: out,
        json: values.json,
        open: !values['no-open'],
        stdin: process.stdin,
        stdout: process.stdout,
        registerDrain: (drain) => {
          owner.drain = drain
        },
      })
      return
    }
    case 'doctor':
      doctor()
      return
    default:
      fail(`unknown command ${JSON.stringify(command)} — run \`cf help\``)
  }
}

main().catch((cause) => {
  fail(cause instanceof Error ? cause.message : String(cause))
})
