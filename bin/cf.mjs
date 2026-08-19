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
import { installCmuxSkills } from '../src/cmux-skills.js'
import { installSkill, skillsStatus, uninstallSkills } from '../src/install.js'
import {
  addParticipant,
  editParticipant,
  listParticipants,
  removeParticipant,
} from '../src/roster.js'
import { generateSkill } from '../src/skill.js'
import { healSkillIfStale, refreshInstalledSkill as refreshSkill } from '../src/sync.js'

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

  setup [--no-cmux] [--force]                  One command: install the cmux skills and, when the
                                               shared roster has participants, the consensflow skill
                                               into every detected coding agent
  participant add <name> --runtime <r> --model <m> [--effort <e>] [--permission workspace-write|full-auto] [--description <d>]
  participant list [--json]
  participant edit <name> [--model <m>] [--effort <e>] [--permission <p>] [--description <d>]
  participant remove <name>
      the roster is the shared ~/.consensflow/participants.json — the same
      file the consensflow-cc plugin and consensflow-pi extension use
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
      force: { type: 'boolean', default: false },
    },
  })

  switch (action) {
    case 'install': {
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

  // cc and pi write the shared roster without telling v3; any invocation is
  // an opportunity to notice and regenerate the installed skill. Skills
  // verbs manage installation explicitly, so they are exempt.
  if (['participant', 'setup', 'ui', 'doctor'].includes(command)) {
    healSkillIfStale(env)
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
