#!/usr/bin/env node
/**
 * The `cf` executable — ConsensFlow v3.
 *
 * v3 is skills-first: there is no delegation engine here. `cf` manages the
 * roster of named agents, generates the consensflow skill from it, and
 * installs/updates that skill — plus cmux's own skills — into every coding
 * harness on the machine (claude, codex, pi, opencode). The skill teaches the
 * harnesses everything else.
 */
import { spawn } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { createInterface } from 'node:readline'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { harnessTurns } from '../hosts/lib/harness-transcript.js'
import { renderImageRun, runImageAgent } from '../hosts/lib/image-run.js'
import { createPacket } from '../hosts/lib/packets.js'
import { interactiveResume, runAgent } from '../hosts/lib/runners.js'
import { runsRoot } from '../hosts/lib/state.js'
import { leadId, loadThreads, newSessionName, saveThread } from '../hosts/lib/threads.js'
import { renderEvent } from '../hosts/lib/transcript-events.js'
import { CATALOG, catalogEntry } from '../src/catalog.js'
import { detectHarnesses } from '../src/harnesses.js'
import { staleClaudeHooks } from '../src/host-payloads.js'
import { installSkill, skillsStatus, skillsSummary, uninstallSkills } from '../src/install.js'
import {
  applyMode,
  currentMode,
  MODES,
  modeLabel,
  modeReport,
  resetEverything,
  resetPreview,
  syncCmuxSkills,
  turnOff,
} from '../src/mode.js'
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
} from '../src/sync.js'
import { terminalRuntime } from '../src/terminal.js'

// `cf … | head` closes our stdout mid-stream; dying with an EPIPE stack for
// that is a crash where a quiet exit is the whole contract of a CLI.
for (const stream of [process.stdout, process.stderr]) {
  stream.on('error', (error) => {
    if (error.code === 'EPIPE') process.exit(0)
    throw error
  })
}

const HERE = dirname(fileURLToPath(import.meta.url))
const PKG = JSON.parse(readFileSync(join(HERE, '..', 'package.json'), 'utf8'))
const env = process.env

const USAGE = `consensflow ${PKG.version}

Usage: cf <command> [options]

  setup [--all] [--force]                      One command: install the cmux skills and, when the
                                               shared roster has agents, the consensflow skill
                                               into every detected coding harness
  use <claude|pi|cmux>                         Who on this machine can consult:
                                               claude / pi = only that harness gets the skill;
                                               cmux (pi, cc, codex, opencode) = every harness gets
                                               it, and cmux's own pane-control skills come with
                                               that mode and only that one
  run <name> "<task>"                          Spawn one agent here and stream its work back:
    [--brief <what this run is for>]            a brief for this spawn, your conversation as
    [--handoff-file <file>] [--no-handoff]      handoff when you pass one, a note alongside it
    [--context <note>] [--prompt-file <file>]
    [--image <path>]                            (image agents: reference pictures)
    [--new] [--session <name>]                  a conversation continues by default in cmux
    [--thread] [--no-thread]                    mode; --new starts a fresh one
  attach <@name|conversation> [--print]        Open the harness's OWN window on that
                                               conversation — the real codex/claude/pi
                                               interface, whole history in it. --print
                                               emits the command instead of running it
  chat <@name|conversation> [--new]            Talk to a conversation instead of commanding
                                               it: one typed line is one turn, /exit or
                                               Ctrl-D leaves, the conversation stays
  sessions [--json]                            The conversations alive in this workspace
  catchup [<name|@agent>] [--last <n>]         Everything said in a conversation, read from
                                               the harness's own session — including turns
                                               you typed yourself after cf attach
  last <name|@agent> [--json]                  The last answer from one of them, and where
                                               its transcript is — how the main pane reads
                                               what happened in an agent's pane
  mode                                         Which one is active, and what it means
  off [--force]                                Take it all back: every file the manifest owns, the
                                               launcher, and the mode. Agents are kept
  reset [--yes]                                The clean slate: everything off removes, plus your
                                               agents and every run artifact. Prints what it would
                                               destroy and refuses without --yes. Cannot be undone
  catalog [--harness <h>] [--json]             The ready-made agents, per harness
  agent add <name>                             A catalog name is enough: cf agent add zeus
  agent add <name> --harness <h> --model <m> [--effort <e>] [--description <d>]
  agent list [--json]
  agent edit <name> [--model <m>] [--effort <e>] [--description <d>]
  agent remove <name>
  agent sync [<name>] [--dry-run]              Re-resolve catalog-backed agents against the
                                               catalog: a preset that moved to a newer model reaches
                                               your roster. Your own definitions and descriptions
                                               are never touched
      the roster is ~/.consensflow/agents.json, shared by every path this
      machine can run (it was participants.json before 2026-08-21 and an
      existing one is still read)
  skills install [--all] [--force]             Generate + install the consensflow skill, and cmux's
                                               own in cmux mode.
                                               Hosts with their own ConsensFlow (the cc plugin, the pi
                                               extension) are left alone unless --all
  skills update [--force]                      Regenerate ours; re-fetch cmux's in cmux mode, take
                                               them back in any other
  skills status                                Every owned file: ok, drifted (user-edited) or missing
  skills uninstall [--force]                   Remove exactly what the manifest owns
  ui [--json] [--no-open]                      Ephemeral local roster editor (Ctrl-C to stop);
                                               --json prints a handle line for a host program
  doctor                                       Harnesses detected, roster size, skills state

Every roster change regenerates the installed consensflow skill. Drifted files
are never overwritten without --force.
`

function out(text) {
  process.stdout.write(`${text}\n`)
}

function fail(message) {
  process.stderr.write(`cf: ${message}\n`)
  process.exitCode = 1
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
 * cmux's own skills, wherever the mode says they belong — which in a host
 * mode is nowhere, so this quietly takes back any that are left over.
 */
function syncCmux(env, values) {
  try {
    const cmux = syncCmuxSkills(env, { force: values.force })
    if (cmux.commit !== null) out(`cmux skills @ ${cmux.commit}`)
    printReport(cmux.report)
  } catch (cause) {
    out(
      `cmux skills were not fetched (${cause instanceof Error ? cause.message.split('(')[0].trim() : cause})`,
    )
  }
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
 * Spawn one agent, here, in whatever mode this machine runs.
 *
 * The host payloads have always had this; the cmux path had only a raw
 * harness command in its skill, which meant no packet, no brief, no handoff
 * and no artifacts. Same verb, same flags, same packet everywhere now — the
 * runner and the packet builder are the shared engine, not a second copy.
 */
/**
 * Which conversation a consult belongs to, and what to hand the runner.
 *
 * The one rule for `cf run` and `cf chat` both — it used to be written twice,
 * and only one copy learned anything. A named session, a deliberately new one,
 * or this lead's own conversation with that agent. Returns `undefined` for
 * `name` when threading is off.
 *
 * `join` is what separates spawning from joining. A consult (`cf run`) takes
 * only what this lead started: standing in a directory where somebody else
 * left a conversation is not a reason to continue it, which is exactly how
 * "ask hyperion for a joke" became turn 4 of an unrelated one. Joining a
 * conversation you can see (`cf chat`, `cf last`, `cf attach`) falls back to
 * the agent's most recent one whoever started it — the user typing in an
 * agent's pane is a different lead by every measure we have, and refusing them
 * their own conversation would be absurd.
 *
 * Naming a session is never scoped: explicit is the user saying which one they
 * mean, and no rule of ours overrules that.
 */
async function resolveConversation(agentRow, { wantsThread, session, fresh, join = false }) {
  if (!wantsThread) return { threads: {}, name: undefined, record: undefined }
  const threads = await loadThreads(cwdOf())
  const nextName = () =>
    newSessionName(
      Object.keys(threads),
      listAgents(env).map((a) => a.name),
    )
  if (session !== undefined) {
    if (threads[session] === undefined) {
      const known = Object.keys(threads)
      return {
        error:
          known.length === 0
            ? `no conversation named ${JSON.stringify(session)} here — omit --session to start one`
            : `no conversation named ${JSON.stringify(session)} here; you have: ${known.join(', ')}`,
      }
    }
    return { threads, name: session, record: threads[session] }
  }
  if (fresh) return { threads, name: nextName(), record: undefined }

  const lead = leadId(env)
  const theirs = Object.entries(threads)
    .filter(([, row]) => row.agent === agentRow.id)
    .sort((a, b) => String(b[1].lastRunAt ?? '').localeCompare(String(a[1].lastRunAt ?? '')))
  // A lead we cannot name is nobody, not everybody: `lead === null` matches no
  // row, so two unidentified shells never share a conversation by accident.
  const mine = lead === null ? [] : theirs.filter(([, row]) => row.lead === lead)
  const picked = mine[0] ?? (join ? theirs[0] : undefined)
  if (picked !== undefined) return { threads, name: picked[0], record: picked[1] }
  return { threads, name: nextName(), record: undefined }
}

/** What to pass runAgent: an object means "this belongs to a conversation". */
function sessionFor(agentRow, name, record) {
  if (name === undefined) return undefined
  return { sessionId: record?.sessionId ?? (agentRow.kind === 'pi' ? name : undefined) }
}

async function recordTurn(name, agentRow, record, result) {
  if (name === undefined) return
  const now = new Date().toISOString()
  await saveThread(cwdOf(), name, {
    agent: agentRow.id,
    kind: agentRow.kind,
    // Whoever started it keeps it. A later turn can come from the user typing
    // in the agent's own pane — a different lead by every measure we have —
    // and rewriting the owner there would take the conversation away from the
    // lead that is still holding it.
    lead: record === undefined ? leadId(env) : (record.lead ?? null),
    sessionId: result.sessionId ?? null,
    runs: (record?.runs ?? 0) + 1,
    createdAt: record?.createdAt ?? now,
    lastRunAt: now,
    lastRunId: result.runId,
  })
}

const cwdOf = () => process.cwd()

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
      thread: { type: 'boolean' },
      'no-thread': { type: 'boolean', default: false },
      new: { type: 'boolean', default: false },
      session: { type: 'string' },
      attach: { type: 'boolean', default: false },
    },
  })

  // An agent must not spawn agents: its own skill would otherwise invite it to.
  if (env.CONSENSFLOW_CHILD === '1') {
    fail('this is already an agent run — an agent does not spawn agents')
    return
  }

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

  const task =
    values['prompt-file'] !== undefined
      ? readFileSync(values['prompt-file'], 'utf8')
      : positionals.slice(1).join(' ')
  if (task.trim().length === 0) {
    fail('give the agent something to do: cf run @name "<task>" (or --prompt-file <file>)')
    return
  }

  // The conversation reaches the agent one way, in every mode: because the
  // lead put it in a file and passed it. Nothing stashes it behind the scenes
  // any more, so nothing differs between the harnesses.
  const handoff =
    values['no-handoff'] || values['handoff-file'] === undefined
      ? ''
      : readFileSync(values['handoff-file'], 'utf8')

  const cwd = process.cwd()

  // An image agent has no harness to launch and no packet to carry: the
  // prompt goes straight to gpt-image-2 through the Codex login.
  if (row.kind === 'image') {
    const result = await runImageAgent({
      cwd,
      agent: row,
      prompt: task,
      imagePaths: values.image ?? [],
    })
    out(values.json ? JSON.stringify(result, null, 2) : renderImageRun(result))
    return
  }

  // Threading: on by default in cmux mode, off in a host mode, and either
  // way overridable. `--session` and `--new` are themselves a request to
  // thread, so naming one is enough.
  const wantsThread =
    values['no-thread'] === true
      ? false
      : (values.thread ?? values.new === true ?? false) ||
        values.session !== undefined ||
        values.new === true ||
        currentMode(env) === 'cmux'

  const resolved = await resolveConversation(row, {
    wantsThread,
    session: values.session,
    fresh: values.new === true,
  })
  if (resolved.error !== undefined) {
    fail(resolved.error)
    return
  }
  const sessionName = resolved.name
  const record = resolved.record
  const started = record === undefined

  // Say which conversation this is on EVERY turn, not only the first. The
  // lead needs the name to read the answer back (`cf last`), to catch up on
  // turns the user took (`cf catchup`) and to find the pane again — and being
  // told it is continuing is what lets it notice the subject has moved on far
  // enough to want `--new`. `--json` is a machine's channel, so it stays clean.
  if (wantsThread && !values.json) {
    out(
      started
        ? `conversation: ${sessionName} (new)`
        : `conversation: ${sessionName} (continuing, turn ${(record.runs ?? 0) + 1})`,
    )
  }

  const packet = await createPacket({
    cwd,
    agent: row,
    kind: 'ask',
    task,
    brief: values.brief,
    extraContext: values.context,
    handoff,
    // A follow-up in a live conversation needs no scene-setting.
    continuing: record?.sessionId !== undefined && record?.sessionId !== null,
    // Threading is what makes a question worth asking: the reply comes back to
    // the same agent rather than to a stranger.
    conversational: wantsThread,
  })

  // Streaming is the point: the thinking has to stay visible while it works.
  // What was streamed is remembered, so the answer is not printed twice when
  // the harness already streamed it — some engines only reveal the answer in
  // the terminal summary, and those still need the block below.
  let inDelta = false
  let sawDelta = false
  let streamed = ''
  const onEvent = values.json
    ? undefined
    : (event) => {
        if (event.kind === 'delta') {
          process.stdout.write(event.text)
          streamed += event.text
          inDelta = true
          sawDelta = true
          return
        }
        if (sawDelta && (event.kind === 'thinking' || event.kind === 'text')) return
        const line = renderEvent(event)
        if (line) {
          process.stdout.write(`${inDelta ? '\n' : ''}${line}\n`)
          streamed += `${line}\n`
          inDelta = false
        }
      }

  let result = await runAgent({
    cwd,
    agent: row,
    packet,
    kind: 'ask',
    onEvent,
    // An object (even an empty one) means "this run belongs to a conversation,
    // so save the session". pi never reports an id back, so we mint one from
    // the conversation's own name — `--session-id` creates it if missing.
    session: wantsThread
      ? { sessionId: record?.sessionId ?? (row.kind === 'pi' ? sessionName : undefined) }
      : undefined,
  })

  // The harness owns the session store and may have pruned it. A conversation
  // it no longer knows costs one fresh start, never the run.
  if (wantsThread && record?.sessionId && result.exitCode !== 0) {
    out('')
    out(`that conversation is gone from ${row.harness ?? row.kind}; starting a new conversation`)
    result = await runAgent({
      cwd,
      agent: row,
      packet,
      kind: 'ask',
      onEvent,
      // Still a conversation — just a new one. Dropping to a one-shot here
      // would make the replacement unresumable too.
      session: { sessionId: row.kind === 'pi' ? `${sessionName}-2` : undefined },
    })
  }

  await recordTurn(sessionName, row, record, result)
  if (inDelta) process.stdout.write('\n')

  // `--attach` leaves the pane as a live agent window: the lead still gets the
  // parsed answer above, and whoever is sitting there can carry on typing in
  // the harness's own interface. Recorded first, so `cf last` works either way.
  let handOverTo = null
  if (values.attach && sessionName !== undefined) {
    const fresh = (await loadThreads(cwd))[sessionName]
    handOverTo = interactiveResume(row, fresh?.sessionId)
  }
  if (values.json) {
    out(JSON.stringify(result, null, 2))
    return
  }
  const answer = (result.output ?? '').trim()
  if (answer.length === 0) {
    out('')
    out(`# @${row.id}`)
    out('')
    out('(no answer)')
    return
  }
  // Already on screen? Then say who it was and stop repeating yourself.
  if (streamed.includes(answer)) {
    out('')
    out(`— @${row.id}`)
    if (handOverTo !== null) await handOver(sessionName, row.id, handOverTo)
    return
  }
  out('')
  out(`# @${row.id}`)
  out('')
  out(answer)
  if (handOverTo !== null) await handOver(sessionName, row.id, handOverTo)
}

function modeVerb() {
  const mode = currentMode(env)
  out(`mode: ${mode === null ? 'not set — nothing is installed yet' : modeLabel(mode)}`)
  for (const line of modeReport(mode ?? 'cmux', env)) out(`  ${line}`)
  if (mode === null) out('')
  if (mode === null) out(`choose one with \`consensflow use <${MODES.join('|')}>\``)
}

/**
 * Everything ConsensFlow installed, taken back: both host payloads, every
 * file the manifest owns, and the mode itself. The roster is the user's and
 * survives — the same contract as the app's "Turn ConsensFlow off".
 */
function resetVerb(rest) {
  const { values } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { yes: { type: 'boolean', default: false } },
  })

  // Counting before refusing makes the refusal the preview: the same two
  // numbers the page puts in its dialog, printed while nothing has been
  // touched. `off` needs no such ceremony — it is undone by choosing a path
  // again. This is not: a roster is typed by hand, and a packet, a transcript
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

/**
 * What conversations exist here, and what the last one said.
 *
 * In cmux mode a consult happens in its own pane, so the answer lands in a run
 * directory rather than in the lead's scrollback. These two verbs are how the
 * main pane reads it — the run directory IS the shared state, which is what
 * stands in for a daemon.
 */
/**
 * A conversation you type into, rather than one you command.
 *
 * The pane an agent runs in is otherwise a place a consult HAPPENED: to ask
 * again you retype `cf run @name "…"` with its quoting. This is the same
 * machinery behind a prompt — each line is one turn in one conversation, the
 * harness session is resumed between them, and every turn still leaves its own
 * run directory. No daemon: the loop is the process you are sitting in, and
 * closing it ends nothing but the typing.
 */
async function chatVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { new: { type: 'boolean', default: false } },
  })

  if (env.CONSENSFLOW_CHILD === '1') {
    fail('this is already an agent run — an agent does not spawn agents')
    return
  }

  const asked = String(positionals[0] ?? '')
  const threads = await loadThreads(cwdOf())
  // Either @agent or a conversation name; a name tells us the agent itself.
  const agentName = asked.startsWith('@') ? asked.slice(1) : (threads[asked]?.agent ?? '')
  const row = agentName.length > 0 ? agentRow(agentName, env) : undefined
  if (row === undefined) {
    const known = listAgents(env).map((a) => a.name)
    fail(
      known.length === 0
        ? 'no agents yet — add one with `cf agent add <name>` or in the app'
        : `name an agent or a conversation; you have: ${known.join(', ')}`,
    )
    return
  }

  const resolved = await resolveConversation(row, {
    wantsThread: true,
    session: asked.startsWith('@') ? undefined : asked,
    fresh: values.new === true,
    // Typing into an agent's own pane is a join, not a spawn: the user is a
    // different lead by every measure we have, and `cf chat @hyperion` there
    // must reach the conversation in front of them, not open a second one.
    join: true,
  })
  if (resolved.error !== undefined) {
    fail(resolved.error)
    return
  }

  let { name, record } = resolved
  out(`${name} · @${row.id}`)
  out('one line is one turn — /exit or Ctrl-D to leave, the conversation stays')
  out('')

  const rl = createInterface({ input: process.stdin, output: process.stdout, prompt: '> ' })
  // A piped stdin can end while a turn is still running, so readline may
  // already be closed by the time we ask for the next line.
  let open = true
  rl.on('close', () => {
    open = false
  })
  const prompt = () => {
    if (open) rl.prompt()
  }
  prompt()
  for await (const line of rl) {
    const task = line.trim()
    if (task === '/exit' || task === '/quit') break
    if (task.length === 0) {
      prompt()
      continue
    }

    const packet = await createPacket({
      cwd: cwdOf(),
      agent: row,
      task,
      kind: 'ask',
      continuing: record?.sessionId !== undefined && record?.sessionId !== null,
      conversational: true,
    })
    const result = await runAgent({
      cwd: cwdOf(),
      agent: row,
      packet,
      kind: 'ask',
      session: sessionFor(row, name, record),
    })
    out('')
    out(String(result.output ?? '').trim() || '(no answer)')
    out('')
    await recordTurn(name, row, record, result)
    record = (await loadThreads(cwdOf()))[name]
    prompt()
  }
  if (open) rl.close()
  out('')
  out(`left ${name} — \`cf chat ${name}\` picks it up again`)
}

/**
 * Hand this terminal to the harness's own window, on the same conversation.
 *
 * `cf chat` is our prompt around one-shot runs; this is the real thing — codex's
 * TUI, claude's, pi's — opened on the session a consult started, with the whole
 * history already in it. We spawn it with the terminal inherited and exit with
 * its code, so from here on ConsensFlow is not in the way at all.
 */
async function attachVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { print: { type: 'boolean', default: false } },
  })
  if (env.CONSENSFLOW_CHILD === '1') {
    fail('this is already an agent run — an agent does not spawn agents')
    return
  }

  const asked = String(positionals[0] ?? '')
  const threads = await loadThreads(cwdOf())
  const names = Object.keys(threads)
  const newest = () =>
    names.sort((a, b) =>
      String(threads[b].lastRunAt ?? '').localeCompare(String(threads[a].lastRunAt ?? '')),
    )[0]
  // Bare `cf attach` means the obvious one: the conversation you were last in.
  const name =
    asked.length === 0
      ? newest()
      : asked.startsWith('@')
        ? names
            .filter((key) => threads[key].agent === asked.slice(1))
            .sort((a, b) =>
              String(threads[b].lastRunAt ?? '').localeCompare(String(threads[a].lastRunAt ?? '')),
            )[0]
        : asked
  const record = name === undefined ? undefined : threads[name]
  if (record === undefined) {
    fail(
      names.length === 0
        ? `no conversation ${JSON.stringify(asked)} here — start one with \`cf run @name "<task>"\``
        : `no conversation ${JSON.stringify(asked)} here; you have: ${names.join(', ')}`,
    )
    return
  }

  const row = agentRow(record.agent, env)
  const invocation = interactiveResume(row ?? { kind: record.kind }, record.sessionId)
  if (invocation === null) {
    fail(
      record.sessionId
        ? `${record.kind} has no interactive session to open`
        : `${name} has no session yet — ask something in it first`,
    )
    return
  }

  const line = [invocation.command, ...invocation.args].join(' ')
  if (values.print) {
    out(line)
    return
  }

  await handOver(name, record.agent, invocation)
}

/** Replace this terminal with the harness's window and exit with its code. */
async function handOver(name, agent, invocation) {
  out(`${name} · @${agent} — handing this terminal to ${invocation.command}`)
  const child = spawn(invocation.command, invocation.args, { cwd: cwdOf(), stdio: 'inherit' })
  const code = await new Promise((resolve) => child.on('close', resolve))
  process.exitCode = code ?? 0
}

/**
 * What was said in a conversation, including turns we never ran.
 *
 * `cf last` reads OUR record of a run. This reads the harness's own session,
 * so a conversation the user took over with `cf attach` is visible too — the
 * lead is no longer blind to it. Read-only, and empty rather than broken if a
 * harness has moved its files.
 */
async function catchupVerb(rest) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { json: { type: 'boolean', default: false }, last: { type: 'string' } },
  })
  const asked = String(positionals[0] ?? '')
  const threads = await loadThreads(cwdOf())
  const names = Object.keys(threads)
  const newest = () =>
    names.sort((a, b) =>
      String(threads[b].lastRunAt ?? '').localeCompare(String(threads[a].lastRunAt ?? '')),
    )[0]
  const name =
    asked.length === 0
      ? newest()
      : asked.startsWith('@')
        ? names
            .filter((key) => threads[key].agent === asked.slice(1))
            .sort((a, b) =>
              String(threads[b].lastRunAt ?? '').localeCompare(String(threads[a].lastRunAt ?? '')),
            )[0]
        : asked
  const record = name === undefined ? undefined : threads[name]
  if (record === undefined) {
    fail(
      names.length === 0
        ? 'no conversations here yet — `cf run @name "<task>"` starts one'
        : `no conversation ${JSON.stringify(asked)} here; you have: ${names.join(', ')}`,
    )
    return
  }

  const turns = await harnessTurns(record.kind, record.sessionId, env)
  if (values.json) {
    out(JSON.stringify({ session: name, agent: record.agent, turns }, null, 2))
    return
  }
  if (turns.length === 0) {
    out(`${name} · @${record.agent} — ${record.kind} keeps no readable transcript for this one`)
    out(`its own runs are still here: cf last ${name}`)
    return
  }
  const limit = Number.parseInt(values.last ?? '0', 10)
  const shown = limit > 0 ? turns.slice(-limit) : turns
  out(`${name} · @${record.agent} · ${turns.length} turns`)
  for (const turn of shown) {
    out('')
    out(turn.role === 'user' ? '› asked' : `• @${record.agent}`)
    out(turn.text)
  }
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
    out(`${name.padEnd(18)}@${String(row.agent).padEnd(12)}${runs.padEnd(9)}${row.lastRunAt ?? ''}`)
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

  // Either a conversation name or an @agent — the agent form resolves to that
  // agent's most recent conversation here.
  let name
  if (wanted.startsWith('@')) {
    const agent = wanted.slice(1)
    const mine = names
      .filter((key) => threads[key].agent === agent)
      .sort((a, b) =>
        String(threads[b].lastRunAt ?? '').localeCompare(String(threads[a].lastRunAt ?? '')),
      )
    name = mine[0]
    if (name === undefined) {
      fail(
        `no conversation with @${agent} here${names.length > 0 ? `; you have: ${names.join(', ')}` : ''}`,
      )
      return
    }
  } else {
    name = wanted
    if (threads[name] === undefined) {
      fail(
        names.length === 0
          ? `no conversation named ${JSON.stringify(wanted)} here — there are none yet`
          : `no conversation named ${JSON.stringify(wanted)} here; you have: ${names.join(', ')}`,
      )
      return
    }
  }

  const row = threads[name]
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
  out(String(result.output ?? '').trim() || '(no answer)')
  out('')
  out(`transcript: ${join(runDir, 'transcript.md')}`)
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

function useVerb(rest) {
  const wanted = rest[0]
  if (!MODES.includes(wanted)) {
    fail(`name a mode: ${MODES.join(', ')}`)
    return
  }
  const outcome = applyMode(wanted, env, {})
  for (const change of outcome.changes) {
    // Name the file whenever there is one: several changes can share a host,
    // and "removed claude integration" twice says less than either path does.
    if (change.path) out(`${(change.action ?? 'changed').padEnd(16)} ${change.path}`)
    else if (change.host) out(`${(change.action ?? 'changed').padEnd(16)} ${change.host}`)
  }
  out('')
  out(`mode: ${modeLabel(outcome.mode)}`)
  for (const line of outcome.report) out(`  ${line}`)
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
    for (const entry of entries) {
      out(
        `  ${entry.name.padEnd(12)}${entry.model.padEnd(34)}${(entry.effort ?? '-').padEnd(8)}${entry.description}`,
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
      // with; this is how a moved preset reaches them. Anything you defined
      // yourself, and any description you wrote, is left alone.
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
      // Installs for whoever the mode puts in scope. It used to refuse in a
      // host mode, which was right while `claude` and `pi` installed a
      // hand-written skill of their own — they are scopes over this same skill
      // now, so refusing would deny them the only skill there is. No mode at
      // all is still a refusal: that is how ConsensFlow used to appear in
      // harnesses nobody had chosen.
      const mode = currentMode(env)
      if (mode === null) {
        fail(
          'no path chosen yet — run `consensflow use cmux` to give every harness the generated skill, or `consensflow use claude|pi` to give it to just that one',
        )
        return
      }
      const agents = listAgents(env)
      if (agents.length === 0) {
        fail('the roster is empty — add an agent with `cf ui` or `cf agent add` first')
        return
      }
      printReport(
        installSkill(
          {
            relPath: 'consensflow/SKILL.md',
            content: generateSkill(agents, { mode: currentMode(env) }),
            source: 'consensflow',
          },
          env,
          { force: values.force, targets: skillTargets(env, { all: values.all }) },
        ),
      )
      reportNativeHosts(env, values.all)
      // In cmux mode its skills come with the install; being offline costs
      // those, not the consensflow skill that just landed.
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
      for (const row of rows) out(`${row.state.padEnd(9)} ${row.source.padEnd(20)} ${row.path}`)
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
          content: generateSkill(agents, { mode: currentMode(env) }),
          source: 'consensflow',
        },
        env,
        { force: values.force, targets: skillTargets(env, { all: values.all }) },
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
  out(`skills:       ${parts.join(' · ')}`)

  // The install records the runtime that performed it — from the app, its own
  // bundled Node. If that has moved, the wiring it left behind stops working,
  // and saying so here is cheaper than letting it fail quietly.
  const wiring = terminalRuntime(env)
  if (wiring !== null) {
    out(
      wiring.exists
        ? `runtime:      ${wiring.runtime}`
        : `runtime:      ${wiring.runtime} — MISSING. Reinstall from the app to point the wiring at its runtime.`,
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
  if (['agent', 'setup', 'ui', 'doctor', 'mode'].includes(command)) {
    healSkillIfStale(env)
  }

  switch (command) {
    case 'use':
      useVerb(rest)
      return
    case 'mode':
      modeVerb()
      return
    case 'catchup':
      await catchupVerb(rest)
      return
    case 'attach':
      await attachVerb(rest)
      return
    case 'chat':
      await chatVerb(rest)
      return
    case 'sessions':
      await sessionsVerb(rest)
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
      await serveUi(env, { onOut: out, json: values.json, open: !values['no-open'] })
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
