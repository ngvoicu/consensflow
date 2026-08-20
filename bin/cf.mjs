#!/usr/bin/env node
/**
 * The `cf` executable — ConsensFlow v3.
 *
 * v3 is skills-first: there is no delegation engine here. `cf` manages the
 * roster of named participants, generates the consensflow skill from it, and
 * installs/updates that skill — plus cmux's own skills — into every coding
 * agent on the machine (claude, codex, pi, opencode). The skill teaches the
 * agents everything else.
 */
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { detectAgents } from '../src/agents.js'
import { CATALOG, catalogEntry } from '../src/catalog.js'
import { installCmuxSkills } from '../src/cmux-skills.js'
import { HOSTS, hostStatus, installHost, uninstallHost } from '../src/hosts.js'
import { installSkill, skillsStatus, uninstallSkills } from '../src/install.js'
import { applyMode, currentMode, MODES, modeReport } from '../src/mode.js'
import {
  addParticipant,
  editParticipant,
  listParticipants,
  removeParticipant,
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

  setup [--no-cmux] [--all] [--force]          One command: install the cmux skills and, when the
                                               shared roster has participants, the consensflow skill
                                               into every detected coding agent
  use <claude|pi|standalone>                   Choose the one path this machine runs:
                                               claude / pi = that agent consults, with your
                                               conversation as context, and no other agent has
                                               ConsensFlow; standalone = every agent can consult
  mode                                         Which one is active, and what it means
  install <claude|pi|all> [--force]            Install a host integration — the deeper path that hands
                                               the participant your live conversation
  uninstall <claude|pi>                        Remove one, exactly as it was installed
  hosts                                        What is installed where
  catalog [--runtime <r>] [--json]              The ready-made participants for each tool
  participant add <name>                       A catalog name is enough: cf participant add zeus
  participant add <name> --runtime <r> --model <m> [--effort <e>] [--description <d>]
  participant list [--json]
  participant edit <name> [--model <m>] [--effort <e>] [--description <d>]
  participant remove <name>
      the roster is the shared ~/.consensflow/participants.json — the same
      file the consensflow-cc plugin and consensflow-pi extension use
  skills install [--with-cmux] [--all] [--force]
                                               Generate + install the consensflow skill (and cmux's).
                                               Hosts with their own ConsensFlow (the cc plugin, the pi
                                               extension) are left alone unless --all
  skills update [--force]                      Regenerate ours; re-fetch cmux's if they are installed
  skills status                                Every owned file: ok, drifted (user-edited) or missing
  skills uninstall [--force]                   Remove exactly what the manifest owns
  ui                                           Ephemeral local roster editor (Ctrl-C to stop)
  doctor                                       Agents detected, roster size, skills state

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
  for (const agent of detectAgents(env).filter((a) => a.native === true)) {
    out(
      `${agent.id}: left alone — ${NATIVE_OWNER[agent.id] ?? 'its own integration'} already provides a consensflow skill (--all to install ours too)`,
    )
  }
}

function printReport(report) {
  for (const row of report) {
    out(`${row.action.padEnd(16)} ${row.path}`)
  }
}

/**
 * A catalog name is a whole participant: `cf participant add zeus` needs no
 * flags. Anything passed explicitly wins over the catalog entry, and a name
 * nobody knows still needs a runtime and a model.
 */
function resolveAdd(name, values) {
  const entry = catalogEntry(name)
  if (entry === undefined && (values.runtime === undefined || values.model === undefined)) {
    throw new Error(
      `${name} is not in the catalog, so it needs --runtime and --model (see \`cf catalog\`)`,
    )
  }
  return {
    name,
    runtime: values.runtime ?? entry?.runtime,
    model: values.model ?? entry?.model,
    effort: values.effort ?? entry?.effort,
    description: values.description ?? entry?.description,
  }
}

function modeVerb() {
  const mode = currentMode(env)
  out(`mode: ${mode ?? 'not set — nothing is installed yet'}`)
  for (const line of modeReport(mode ?? 'standalone', env)) out(`  ${line}`)
  if (mode === null) out('')
  if (mode === null) out(`choose one with \`consensflow use <${MODES.join('|')}>\``)
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
  out(`mode: ${outcome.mode}`)
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
    options: { runtime: { type: 'string' }, json: { type: 'boolean', default: false } },
  })

  const catalog =
    values.runtime === undefined ? CATALOG : { [values.runtime]: CATALOG[values.runtime] ?? [] }

  if (values.json) {
    out(JSON.stringify({ catalog }, null, 2))
    return
  }
  for (const [runtime, entries] of Object.entries(catalog)) {
    out(`${runtime}:`)
    for (const entry of entries) {
      out(
        `  ${entry.name.padEnd(12)}${entry.model.padEnd(34)}${(entry.effort ?? '-').padEnd(8)}${entry.description}`,
      )
    }
    out('')
  }
  out('add one with `cf participant add <name>` — no other flags needed')
}

function participantVerb(rest) {
  const action = rest[0]
  const { values, positionals } = parseArgs({
    args: rest.slice(1),
    allowPositionals: true,
    options: {
      runtime: { type: 'string' },
      model: { type: 'string' },
      effort: { type: 'string' },
      description: { type: 'string' },
      from: { type: 'string' },
      presets: { type: 'string' },
      json: { type: 'boolean', default: false },
    },
  })
  const name = positionals[0]

  switch (action) {
    case 'add': {
      const added = addParticipant(resolveAdd(name, values), env)
      refreshSkill(env)
      out(`${added.name}  ${added.runtime}  ${added.model}`)
      return
    }
    case 'list': {
      const participants = listParticipants(env)
      if (values.json) {
        out(JSON.stringify({ participants }, null, 2))
        return
      }
      if (participants.length === 0) {
        out('no participants yet — add one with `cf ui` or `cf participant add`')
        return
      }
      for (const p of participants) {
        out(`${p.name.padEnd(14)}${p.runtime.padEnd(10)}${p.model.padEnd(36)}${p.effort ?? '-'}`)
      }
      return
    }
    case 'edit': {
      const edited = editParticipant(
        name,
        {
          ...(values.model !== undefined ? { model: values.model } : {}),
          ...(values.effort !== undefined ? { effort: values.effort } : {}),
          ...(values.description !== undefined ? { description: values.description } : {}),
        },
        env,
      )
      refreshSkill(env)
      out(`${edited.name}  ${edited.runtime}  ${edited.model}`)
      return
    }
    case 'remove': {
      removeParticipant(name, env)
      refreshSkill(env)
      out(`removed ${name}`)
      return
    }
    default:
      fail('usage: cf participant add|list|edit|remove')
  }
}

function skillsVerb(rest) {
  const action = rest[0]
  const { values } = parseArgs({
    args: rest.slice(1),
    allowPositionals: true,
    options: {
      'with-cmux': { type: 'boolean', default: false },
      all: { type: 'boolean', default: false },
      force: { type: 'boolean', default: false },
    },
  })

  switch (action) {
    case 'install': {
      const mode = currentMode(env)
      if (mode !== null && mode !== 'standalone') {
        fail(
          `this machine is in ${mode} mode, where only ${mode} consults — run \`consensflow use standalone\` to give every agent the generated skill`,
        )
        return
      }
      const participants = listParticipants(env)
      if (participants.length === 0) {
        fail('the roster is empty — add a participant with `cf ui` or `cf participant add` first')
        return
      }
      printReport(
        installSkill(
          {
            relPath: 'consensflow/SKILL.md',
            content: generateSkill(participants),
            source: 'consensflow',
          },
          env,
          { force: values.force, targets: skillTargets(env, { all: values.all }) },
        ),
      )
      reportNativeHosts(env, values.all)
      if (values['with-cmux']) {
        const cmux = installCmuxSkills(env, { force: values.force })
        out(`cmux skills @ ${cmux.commit}`)
        printReport(cmux.report)
      }
      return
    }
    case 'update': {
      refreshSkill(env)
      const hasCmux = skillsStatus(env).some((row) => row.source.startsWith('cmux@'))
      if (hasCmux) {
        const cmux = installCmuxSkills(env, { force: values.force })
        out(`cmux skills @ ${cmux.commit}`)
        printReport(cmux.report)
      }
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
      'no-cmux': { type: 'boolean', default: false },
      all: { type: 'boolean', default: false },
      force: { type: 'boolean', default: false },
    },
  })

  const agents = detectAgents(env)
  out(
    agents.length > 0
      ? `agents: ${agents.map((a) => a.id).join(', ')}`
      : 'agents: none found on PATH — install claude, codex, pi or opencode and rerun',
  )

  const participants = listParticipants(env)
  if (participants.length === 0) {
    // Participants are the user's to create; nothing is seeded for them.
    out(
      'participants: none yet — create them with `cf ui` (or `cf participant add`); the skill installs itself on the first one',
    )
  } else if (agents.length > 0) {
    printReport(
      installSkill(
        {
          relPath: 'consensflow/SKILL.md',
          content: generateSkill(participants),
          source: 'consensflow',
        },
        env,
        { force: values.force, targets: skillTargets(env, { all: values.all }) },
      ),
    )
    reportNativeHosts(env, values.all)
  }

  if (!values['no-cmux'] && agents.length > 0) {
    const cmux = installCmuxSkills(env, { force: values.force })
    out(`cmux skills @ ${cmux.commit}`)
    printReport(cmux.report)
  }
}

function doctor() {
  const agents = detectAgents(env)
  out(`consensflow ${PKG.version}`)
  out(
    `agents:       ${agents.length > 0 ? agents.map((a) => `${a.id}${a.native ? ' (has its own consensflow)' : ''}`).join(', ') : 'none on PATH'}`,
  )
  out(`participants: ${listParticipants(env).length}`)
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
  if (['participant', 'setup', 'ui', 'doctor', 'mode'].includes(command)) {
    healSkillIfStale(env)
  }

  switch (command) {
    case 'use':
      useVerb(rest)
      return
    case 'mode':
      modeVerb()
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
    case 'participant':
      participantVerb(rest)
      return
    case 'skills':
      skillsVerb(rest)
      return
    case 'setup':
      setup(rest)
      return
    case 'ui': {
      const { serveUi } = await import('../src/ui.js')
      await serveUi(env, { onOut: out })
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
