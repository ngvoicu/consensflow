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
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { detectAgents } from '../src/agents.js'
import { installCmuxSkills } from '../src/cmux-skills.js'
import { installSkill, skillsStatus, uninstallSkills } from '../src/install.js'
import {
  addParticipant,
  editParticipant,
  importV1,
  listParticipants,
  removeParticipant,
} from '../src/roster.js'
import { generateSkill } from '../src/skill.js'

const HERE = dirname(fileURLToPath(import.meta.url))
const PKG = JSON.parse(readFileSync(join(HERE, '..', 'package.json'), 'utf8'))
const env = process.env

const USAGE = `consensflow ${PKG.version}

Usage: cf <command> [options]

  setup [--no-cmux] [--force]                  One command: import a v1 roster if found, generate the
                                               consensflow skill, install it (and cmux's skills) into
                                               every detected coding agent
  participant add <name> --runtime <r> --model <m> [--effort <e>] [--permission workspace-write|full-auto] [--description <d>]
  participant list [--json]
  participant edit <name> [--model <m>] [--effort <e>] [--permission <p>] [--description <d>]
  participant remove <name>
  participant import-v1 [--from <path>] [--presets <path>]
  skills install [--with-cmux] [--force]       Generate + install the consensflow skill (and cmux's)
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

function printReport(report) {
  for (const row of report) {
    out(`${row.action.padEnd(16)} ${row.path}`)
  }
}

/** Regenerates + reinstalls the consensflow skill wherever it is installed. */
function refreshInstalledSkill() {
  const owned = skillsStatus(env).filter((row) => row.source === 'consensflow')
  if (owned.length === 0) return
  const participants = listParticipants(env)
  if (participants.length === 0) return
  installSkill(
    {
      relPath: 'consensflow/SKILL.md',
      content: generateSkill(participants),
      source: 'consensflow',
    },
    env,
  )
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
      permission: { type: 'string' },
      description: { type: 'string' },
      from: { type: 'string' },
      presets: { type: 'string' },
      json: { type: 'boolean', default: false },
    },
  })
  const name = positionals[0]

  switch (action) {
    case 'add': {
      const added = addParticipant(
        {
          name,
          runtime: values.runtime,
          model: values.model,
          effort: values.effort,
          permission: values.permission,
          description: values.description,
        },
        env,
      )
      refreshInstalledSkill()
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
        out(
          'no participants yet — add one with `cf participant add` or import v1 with `cf participant import-v1`',
        )
        return
      }
      for (const p of participants) {
        out(
          `${p.name.padEnd(14)}${p.runtime.padEnd(10)}${p.model.padEnd(36)}${(p.effort ?? '-').padEnd(8)}${p.permission}`,
        )
      }
      return
    }
    case 'edit': {
      const edited = editParticipant(
        name,
        {
          ...(values.model !== undefined ? { model: values.model } : {}),
          ...(values.effort !== undefined ? { effort: values.effort } : {}),
          ...(values.permission !== undefined ? { permission: values.permission } : {}),
          ...(values.description !== undefined ? { description: values.description } : {}),
        },
        env,
      )
      refreshInstalledSkill()
      out(`${edited.name}  ${edited.runtime}  ${edited.model}`)
      return
    }
    case 'remove': {
      removeParticipant(name, env)
      refreshInstalledSkill()
      out(`removed ${name}`)
      return
    }
    case 'import-v1': {
      const v1Path = values.from ?? join(env.HOME ?? homedir(), '.consensflow', 'participants.json')
      const outcome = importV1({ v1Path, presetsPath: values.presets }, env)
      refreshInstalledSkill()
      out(`imported ${outcome.imported.length}: ${outcome.imported.map((p) => p.name).join(', ')}`)
      for (const s of outcome.skipped) out(`skipped ${s.name}: ${s.reason}`)
      return
    }
    default:
      fail('usage: cf participant add|list|edit|remove|import-v1')
  }
}

function skillsVerb(rest) {
  const action = rest[0]
  const { values } = parseArgs({
    args: rest.slice(1),
    allowPositionals: true,
    options: {
      'with-cmux': { type: 'boolean', default: false },
      force: { type: 'boolean', default: false },
    },
  })

  switch (action) {
    case 'install': {
      const participants = listParticipants(env)
      if (participants.length === 0) {
        fail('the roster is empty — add a participant (or `cf participant import-v1`) first')
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
          { force: values.force },
        ),
      )
      if (values['with-cmux']) {
        const cmux = installCmuxSkills(env, { force: values.force })
        out(`cmux skills @ ${cmux.commit}`)
        printReport(cmux.report)
      }
      return
    }
    case 'update': {
      refreshInstalledSkill()
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
      force: { type: 'boolean', default: false },
    },
  })

  // A v1 roster on this machine is curation worth keeping: import it once,
  // stated — never silently on a roster that already has entries.
  const v1Path = join(env.HOME ?? homedir(), '.consensflow', 'participants.json')
  if (listParticipants(env).length === 0 && existsSync(v1Path)) {
    const outcome = importV1({ v1Path, presetsPath: v1PresetsPath() }, env)
    out(
      `imported ${outcome.imported.length} v1 participants: ${outcome.imported.map((p) => p.name).join(', ')}`,
    )
    for (const s of outcome.skipped) out(`skipped ${s.name}: ${s.reason}`)
  }

  const agents = detectAgents(env)
  out(
    agents.length > 0
      ? `agents: ${agents.map((a) => a.id).join(', ')}`
      : 'agents: none found on PATH — install claude, codex, pi or opencode and rerun',
  )

  const participants = listParticipants(env)
  if (participants.length === 0) {
    out(
      'participants: none — add some with `cf participant add` or `cf ui`, then rerun `cf skills install`',
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
        { force: values.force },
      ),
    )
  }

  if (!values['no-cmux'] && agents.length > 0) {
    const cmux = installCmuxSkills(env, { force: values.force })
    out(`cmux skills @ ${cmux.commit}`)
    printReport(cmux.report)
  }
}

/** The v1 Claude Code plugin's presets.js, when that plugin is installed. */
function v1PresetsPath() {
  const base = join(
    env.HOME ?? homedir(),
    '.claude',
    'plugins',
    'cache',
    'consensflow-cc',
    'consensflow',
  )
  try {
    const versions = readdirSync(base).sort()
    const newest = versions[versions.length - 1]
    if (newest === undefined) return undefined
    const candidate = join(base, newest, 'lib', 'presets.js')
    return existsSync(candidate) ? candidate : undefined
  } catch {
    return undefined
  }
}

function doctor() {
  const agents = detectAgents(env)
  out(`consensflow ${PKG.version}`)
  out(`agents:       ${agents.length > 0 ? agents.map((a) => a.id).join(', ') : 'none on PATH'}`)
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

  switch (command) {
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
