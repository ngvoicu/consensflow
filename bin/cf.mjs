#!/usr/bin/env node
/** App-scoped conversation commands, saved roster administration and runtime diagnostics. */
import { spawn, spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs } from 'node:util'
import { CATALOG, catalogEntry } from '../src/catalog.js'
import { detectHarnesses } from '../src/harnesses.js'
import { staleClaudeHooks } from '../src/host-payloads.js'
import { prepareApp } from '../src/install.js'
import { addAgent, configRoot, editAgent, listAgents, removeAgent } from '../src/roster.js'
import { terminalRuntime } from '../src/terminal.js'

// `cf … | head` closes our stdout mid-stream; dying with an EPIPE stack for
// that is a crash where a quiet exit is the whole contract of a CLI.
//
// The daemon (`cf ui`) is the one verb that owns durable state, so it installs a drain
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

  setup                                     Prepare private launcher and integrations
  catalog [--harness <h>] [--json]            List available agent presets
  agent add <name>                           Add an agent of your own
    [--harness <h>] [--model <m>] [--effort <e>] [--description <d>]
    [--designer]                            An image agent (Codex only): an image designer
  agent list [--json]
  agent edit <name> [--model <m>] [--effort <e>] [--description <d>]
    [--work-tier critical|complex|standard|light|auto]
  agent remove <name>
  ui [--json] [--no-open]                     Run the app's daemon; open the agents screens
  doctor                                    Inspect runtime, roster and bundled roles

Inside a window ConsensFlow opened, cf is the board: task add --tier <t> "…",
task list|get|done|accept|reopen|cancel|pause|resume, tell, inbox, ask, note,
answer, staff, whoami, history (cf help there says more).
`

function out(text) {
  process.stdout.write(`${text}\n`)
}

function fail(message) {
  process.stderr.write(`cf: ${message}\n`)
  process.exitCode = 1
}

/**
 * An agent of the human's own: a name, a harness and a model. A catalog
 * agent is in the roster already, so its name is refused; `--designer`
 * makes it an image agent, which only a Codex agent can be.
 */
function resolveAdd(name, values) {
  // Every catalog agent is in the roster already; an add defines one by hand.
  if (catalogEntry(name) !== undefined) {
    throw new Error(
      `${name} is a catalog agent, in your agents already: pick another name for your own`,
    )
  }
  if (values.harness === undefined || values.model === undefined) {
    throw new Error(
      `${name} needs --harness and --model (see \`cf catalog\` for the ready-made ones)`,
    )
  }
  return {
    name,
    harness: values.harness,
    model: values.model,
    effort: values.effort,
    description: values.description,
    ...(values.designer ? { designer: true } : {}),
  }
}

// --- standalone: ConsensFlow's own app owns the panes -------------------------

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
  out(
    'every one of them is in your agents already; define your own with `cf agent add <name> --harness … --model …`',
  )
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
      'work-tier': { type: 'string' },
      description: { type: 'string' },
      designer: { type: 'boolean', default: false },
      json: { type: 'boolean', default: false },
    },
  })
  const name = positionals[0]

  switch (action) {
    case 'add': {
      const added = addAgent(
        {
          ...resolveAdd(name, values),
          ...(values['work-tier'] === undefined || values['work-tier'] === 'auto'
            ? {}
            : { workTier: values['work-tier'] }),
        },
        env,
      )
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
          ...(values['work-tier'] === undefined
            ? {}
            : { workTier: values['work-tier'] === 'auto' ? null : values['work-tier'] }),
        },
        env,
      )
      out(`${edited.name}  ${edited.harness}  ${edited.model}`)
      return
    }
    case 'remove': {
      removeAgent(name, env)
      out(`removed ${name}`)
      return
    }
    default:
      fail('usage: cf agent add|list|edit|remove')
  }
}

function setup(rest) {
  parseArgs({ args: rest, allowPositionals: false, options: {} })
  const installed = prepareApp(env)
  for (const line of installed.report) out(line)
  const harnesses = detectHarnesses(env)
  out(
    `harnesses: ${harnesses.length ? harnesses.map((h) => h.id).join(', ') : 'none found on PATH'}`,
  )
  out(`agents: ${listAgents(env).length} saved — manage them with cf ui or cf agent`)
}

function doctor() {
  const harnesses = detectHarnesses(env)
  out(`consensflow ${PKG.version}`)
  out(`home:         ${configRoot(env)}`)
  out(
    `harnesses:    ${harnesses.length > 0 ? harnesses.map((a) => a.id).join(', ') : 'none on PATH'}`,
  )
  out(`agents:       ${listAgents(env).length}`)
  out('roles:        bundled chief, worker, reviewer and advisor; prepared when a window launches')

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
  // A window the daemon opened carries its participant's token; there, `cf`
  // is the board's commands, which the native `cf` beside this file answers
  // (crates/cf). A window finds that one first on its PATH; this is for
  // whoever runs cf.mjs itself, as the terminal's launcher does.
  if (env.CONSENSFLOW_TOKEN) {
    const native = join(HERE, process.platform === 'win32' ? 'cf.exe' : 'cf')
    const ran = spawnSync(native, process.argv.slice(2), { stdio: 'inherit' })
    if (ran.error) return fail(`${native} did not start: ${ran.error.message}`)
    process.exitCode = ran.status ?? 1
    return
  }

  if (command === undefined || command === 'help' || command === '--help') {
    out(USAGE)
    return
  }
  if (command === '--version' || command === '-v' || command === 'version') {
    out(PKG.version)
    return
  }

  switch (command) {
    case 'catalog':
      catalogVerb(rest)
      return
    case 'agent':
      agentVerb(rest)
      return
    case 'setup':
      setup(rest)
      return
    case 'ui': {
      // The app's daemon. `--json` prints the handle line the
      // app reads; a person gets the agents screens' address instead.
      const { startDaemon } = await import('../src/core/daemon.js')
      const { values } = parseArgs({
        args: rest,
        allowPositionals: true,
        options: {
          json: { type: 'boolean', default: false },
          'no-open': { type: 'boolean', default: false },
        },
      })
      const { stop } = await startDaemon(env, {
        onOut: (line) => {
          if (values.json) return out(line)
          const { url, token } = JSON.parse(line)
          const address = `${url}?token=${token}`
          out(`agents: ${address}`)
          out('Ctrl-C to stop — nothing keeps running after it.')
          if (!values['no-open'])
            spawn('open', [address], { stdio: 'ignore', detached: true }).unref()
        },
      })
      owner.drain = stop
      return new Promise(() => {})
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
