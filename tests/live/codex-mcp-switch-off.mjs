/**
 * Live: does the real Codex take the flags that switch off its MCP servers?
 *
 * A member's window switches off every MCP server Codex would start, each in
 * the form of its own transport (`mcpIsolation`, src/adapters/codex.js). A
 * command on a server Codex reaches by URL is a config it refuses ("url is not
 * supported for stdio"): the app-server under the window exited, and every
 * Codex window of a Mac that had one closed within a tenth of a second
 * (2026-10-06, Codex 0.160.1). This gives the installed Codex a home of its
 * own with a server of each transport, asks it to list them, switches them
 * off as the adapter does, and needs Codex to take the flags and list each
 * server disabled: a command's on the harmless command, a URL's on the discard
 * URL. It then gives Codex the old flags, a command on every server, and says
 * what Codex made of them (a note, not a verdict: Codex may one day take them).
 * `codex mcp list` only reads its config, so no model is asked; the home is
 * the check's own, and the URLs are on loopback's discard port.
 *
 *   npm run live:codex-mcp
 *
 * The exit code is 1 when Codex lists a transport other than the one the
 * adapter reads, refuses the flags, or lists a server not switched off.
 */
import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { mcpIsolation } from '../../src/adapters/codex.js'
import { harnessPath, runnable } from '../../src/harnesses.js'

const executable = harnessPath('codex', process.env)
if (executable === null) {
  process.stdout.write('left out: codex is not installed on this machine\n')
  process.exit(0)
}

/** How Codex lists each server of the check's config: one by command, two by URL. */
const TRANSPORTS = { local: 'stdio', idea: 'streamable_http', authd: 'streamable_http' }
const COMMAND = '/usr/bin/true'
const DISABLED_URL = 'http://127.0.0.1:9/disabled'

const root = realpathSync.native(mkdtempSync(path.join(os.tmpdir(), 'consensflow codex mcp-')))
const at = (...parts) => path.join(root, ...parts)
const env = {
  ...process.env,
  HOME: at('home'),
  USERPROFILE: at('home'),
  CODEX_HOME: at('codex'),
  XDG_CONFIG_HOME: at('xdg', 'config'),
  XDG_DATA_HOME: at('xdg', 'data'),
  XDG_CACHE_HOME: at('xdg', 'cache'),
  XDG_STATE_HOME: at('xdg', 'state'),
  APPDATA: at('appdata', 'roaming'),
  LOCALAPPDATA: at('appdata', 'local'),
  TMPDIR: at('tmp'),
  TMP: at('tmp'),
  TEMP: at('tmp'),
}

/** Codex's answer to `args`: how it ended and what it said. */
function ask(args) {
  const run = runnable(executable, args, env)
  const done = spawnSync(run.file, run.args, {
    ...run.options,
    env,
    encoding: 'utf8',
    timeout: 30_000,
  })
  return {
    status: done.status,
    stdout: done.stdout ?? '',
    stderr: `${done.stderr ?? ''}${done.error?.message ?? ''}`.trim(),
  }
}

/** The servers `codex mcp list --json` names with `flags` before it, or why not. */
function listed(flags) {
  const done = ask([...flags, 'mcp', 'list', '--json'])
  if (done.status !== 0) return { refused: `exit ${done.status}: ${done.stderr}` }
  try {
    return { servers: JSON.parse(done.stdout) }
  } catch {
    return { refused: `an answer that is no JSON: ${done.stdout.slice(0, 200)}` }
  }
}

/** What Codex lists of each server, as a line: `name (transport)`. */
const typed = (servers) => servers.map(({ name, transport }) => `${name} (${transport?.type})`)
const flat = (text) => text.replace(/\s+/g, ' ')

const report = []
const say = (verdict, text) => report.push(`${verdict} ${text}`)
let failed = false
const fail = (text) => {
  failed = true
  say('FAIL', text)
}
try {
  for (const folder of ['home', 'codex', 'xdg', 'appdata', 'tmp']) mkdirSync(at(folder))
  writeFileSync(
    at('codex', 'config.toml'),
    `[mcp_servers.local]
command = '${process.execPath}'
args = ['server']

[mcp_servers.idea]
url = "http://127.0.0.1:9/mcp"

[mcp_servers.authd]
url = "http://127.0.0.1:9/mcp"
bearer_token_env_var = "CF_LIVE_TOKEN"
http_headers = { "X-A" = "b" }
`,
  )
  const version = ask(['--version']).stdout.trim() || 'codex'

  const before = listed([])
  if (before.refused !== undefined) {
    fail(`${version} could not list its servers: ${flat(before.refused)}`)
  } else {
    const kinds = Object.fromEntries(
      before.servers.map(({ name, transport }) => [name, transport?.type]),
    )
    const odd = Object.entries(TRANSPORTS).filter(([name, type]) => kinds[name] !== type)
    if (odd.length > 0) {
      fail(
        `${version} lists ${typed(before.servers).join(', ')}, not ${JSON.stringify(TRANSPORTS)}`,
      )
    } else {
      say('ok  ', `${version} lists ${typed(before.servers).join(', ')}`)
    }

    const after = listed(mcpIsolation(before.servers))
    if (after.refused !== undefined) {
      fail(`${version} refused the flags that switch its servers off: ${flat(after.refused)}`)
    } else {
      const problems = []
      for (const { name, enabled, transport } of after.servers) {
        const type = kinds[name]
        if (enabled !== false) problems.push(`${name} is still enabled`)
        if (transport?.type !== type) {
          problems.push(`${name} was listed as ${type} and is now ${transport?.type}`)
        } else if (type === 'stdio' && transport.command !== COMMAND) {
          problems.push(`${name} runs ${transport.command}, not ${COMMAND}`)
        } else if (type !== 'stdio' && transport.url !== DISABLED_URL) {
          problems.push(`${name} is at ${transport.url}, not ${DISABLED_URL}`)
        }
      }
      const states = after.servers.map(({ name, enabled }) => `${name} ${enabled ? 'on' : 'off'}`)
      if (problems.length > 0) fail(`${version} lists ${states.join(', ')}: ${problems.join('; ')}`)
      else
        say(
          'ok  ',
          `${version} took the flags that switch them off, and lists ${states.join(', ')}`,
        )
    }

    // What the adapter made before: a command on every server, a URL's too.
    const old = listed(
      before.servers.flatMap(({ name }) => [
        '-c',
        `mcp_servers.${name}.command="${COMMAND}"`,
        '-c',
        `mcp_servers.${name}.enabled=false`,
      ]),
    )
    say(
      'note',
      old.refused === undefined
        ? `${version} takes the old flags, a command on every server, too`
        : `${version} refuses the old flags, a command on every server: ${flat(old.refused)}`,
    )
  }
} finally {
  rmSync(root, { recursive: true, force: true })
}

for (const line of report) process.stdout.write(`${line}\n`)
process.exit(failed ? 1 : 0)
