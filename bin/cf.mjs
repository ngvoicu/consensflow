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
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { renderImageRun, runImageAgent } from '../hosts/lib/image-run.js'
import { createPacket } from '../hosts/lib/packets.js'
import { runAgent } from '../hosts/lib/runners.js'
import { renderEvent } from '../hosts/lib/transcript-events.js'
import { CATALOG, catalogEntry } from '../src/catalog.js'
import { detectHarnesses } from '../src/harnesses.js'
import { HOSTS, hostStatus, installHost, uninstallHost } from '../src/hosts.js'
import { installSkill, skillsStatus, uninstallSkills } from '../src/install.js'
import {
  applyMode,
  currentMode,
  MODES,
  modeLabel,
  modeReport,
  syncCmuxSkills,
  turnOff,
} from '../src/mode.js'
import {
  addAgent,
  agentRow,
  editAgent,
  listAgents,
  removeAgent,
  syncAgents,
} from '../src/roster.js'
import { generateSkill } from '../src/skill.js'
import {
  healSkillIfStale,
  refreshInstalledSkill as refreshSkill,
  retireSkillFromNativeHosts,
  skillTargets,
} from '../src/sync.js'

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
  use <claude|pi|cmux>                         Choose the one path this machine runs:
                                               claude / pi = that harness consults, with your
                                               conversation as context, and no other harness has
                                               ConsensFlow; cmux (pi, cc, codex, opencode) = every
                                               harness can consult, and cmux's own pane-control skills
                                               come with that mode and only that one
  run <name> "<task>"                          Spawn one agent here and stream its work back:
    [--brief <what this run is for>]            a brief for this spawn, your conversation as
    [--handoff-file <file>] [--no-handoff]      handoff when you pass one, a note alongside it
    [--context <note>] [--prompt-file <file>]
    [--image <path>]                            (image agents: reference pictures)
  mode                                         Which one is active, and what it means
  off [--force]                                Take it all back: both host payloads, every file the
                                               manifest owns, and the mode. Agents are kept
  install <claude|pi|all> [--force]            Install a host integration — the deeper path that hands
                                               your live conversation to the agent
  uninstall <claude|pi>                        Remove one, exactly as it was installed
  hosts                                        What is installed where
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

  // Handoff is the lead's to give: from a file it names, or not at all. In a
  // host mode the integration stashes the session and passes it the same way.
  const handoff =
    values['no-handoff'] === true || values['handoff-file'] === undefined
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

  const packet = await createPacket({
    cwd,
    agent: row,
    kind: 'ask',
    task,
    brief: values.brief,
    extraContext: values.context,
    handoff,
  })

  // Streaming is the point: the thinking has to stay visible while it works.
  let inDelta = false
  let sawDelta = false
  const onEvent = values.json
    ? undefined
    : (event) => {
        if (event.kind === 'delta') {
          process.stdout.write(event.text)
          inDelta = true
          sawDelta = true
          return
        }
        if (sawDelta && (event.kind === 'thinking' || event.kind === 'text')) return
        const line = renderEvent(event)
        if (line) {
          process.stdout.write(`${inDelta ? '\n' : ''}${line}\n`)
          inDelta = false
        }
      }

  const result = await runAgent({ cwd, agent: row, packet, kind: 'ask', onEvent })
  if (inDelta) process.stdout.write('\n')
  if (values.json) {
    out(JSON.stringify(result, null, 2))
    return
  }
  out('')
  out(`# @${row.id}`)
  out('')
  out(result.output ?? '(no answer)')
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
    if (change.action === 'removed' && change.host)
      out(`removed          ${change.host} integration`)
    else if (change.path) out(`${(change.action ?? 'changed').padEnd(16)} ${change.path}`)
  }
  out('')
  out(`mode: ${modeLabel(outcome.mode)}`)
  for (const line of outcome.report) out(`  ${line}`)
}

function hostsVerb() {
  for (const host of hostStatus(env)) {
    const detail = host.installed
      ? `installed by consensflow${host.version ? ` (v${host.version})` : ''}${host.files ? ` · ${host.files} files wired` : ''}`
      : host.present
        ? `already present via ${host.via} — consensflow did not install it and will not remove it`
        : 'not installed'
    out(`${host.id.padEnd(8)}${detail}`)
  }
  out('')
  out('install one with `consensflow install <host>`; the roster is shared with every host')
}

function installVerb(rest, remove = false) {
  const { values, positionals } = parseArgs({
    args: rest,
    allowPositionals: true,
    options: { force: { type: 'boolean', default: false } },
  })
  const wanted = positionals[0]
  if (wanted === undefined || (wanted !== 'all' && !HOSTS.includes(wanted))) {
    fail(`name a host to ${remove ? 'uninstall' : 'install'}: ${HOSTS.join(', ')}`)
    return
  }
  const targets = wanted === 'all' ? HOSTS : [wanted]
  for (const host of targets) {
    const outcome = remove
      ? uninstallHost(host, env)
      : installHost(host, env, { force: values.force })
    out(
      remove
        ? `${host}: removed`
        : `${host}: installed${outcome.version ? ` (v${outcome.version})` : ''}`,
    )
  }
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
      // The generated skill belongs to cmux mode. The guard used to test for
      // the mode's old name, so it refused in cmux mode and allowed the one
      // case that should never install: no mode at all, which is how
      // ConsensFlow appeared in harnesses nobody had chosen.
      const mode = currentMode(env)
      if (mode !== 'cmux') {
        fail(
          mode === null
            ? 'no path chosen yet — run `consensflow use cmux` to give every harness the generated skill, or `consensflow use claude|pi` for the deeper integration in one of them'
            : `this machine is in ${mode} mode, where only ${mode} consults — run \`consensflow use cmux\` to give every harness the generated skill`,
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
            content: generateSkill(agents),
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
          content: generateSkill(agents),
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
  out(
    `harnesses:       ${harnesses.length > 0 ? harnesses.map((a) => `${a.id}${a.native ? ' (has its own consensflow)' : ''}`).join(', ') : 'none on PATH'}`,
  )
  out(`agents: ${listAgents(env).length}`)
  const rows = skillsStatus(env)
  const drifted = rows.filter((r) => r.state !== 'ok').length
  out(
    `skills:       ${rows.length} owned files${drifted > 0 ? `, ${drifted} drifted/missing` : ''}`,
  )
}

async function main() {
  const [command, ...rest] = process.argv.slice(2)

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
    case 'off':
      offVerb(rest)
      return
    case 'run':
      await runVerb(rest)
      return
    case 'hosts':
      hostsVerb()
      return
    case 'install':
      installVerb(rest)
      return
    case 'uninstall':
      installVerb(rest, true)
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
