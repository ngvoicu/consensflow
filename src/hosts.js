import { spawnSync } from 'node:child_process'
import {
  cpSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { configRoot } from './roster.js'

/**
 * Host integrations: the deeper ConsensFlow paths that live inside a coding
 * agent and can hand it your live conversation.
 *
 * One manager installs them all, so there is a single place to install,
 * update and remove — and a single roster behind them. Each host is driven
 * through a surface it actually supports:
 *
 * - **pi** has a real CLI (`pi install <source>`), so we use it. Nothing is
 *   written by hand into pi's state.
 * - **Claude Code** has no install CLI, and its plugin registry is versioned
 *   internal state. So the payload lives in OUR directory
 *   (`<config>/hosts/claude`) and is wired up through documented user config:
 *   a skill, a command, and hook entries in settings.json. Everything we
 *   write is recorded, and uninstall removes exactly that.
 */

const PI_SOURCE = 'github.com/ngvoicu/consensflow-pi'
const HOOK_TAG = 'consensflow'

export const HOSTS = ['claude', 'pi']

function claudeConfigDir(env) {
  return env.CLAUDE_CONFIG_DIR ?? join(env.HOME ?? homedir(), '.claude')
}

function payloadDir(env) {
  return join(configRoot(env), 'hosts', 'claude')
}

/**
 * The engine, installed beside the payloads rather than inside one.
 *
 * Both payloads reach it as a sibling — `../lib` from pi's `index.ts`,
 * `../../lib` from Claude Code's `bin/cf.mjs` and hooks — because that is
 * the shape of the repo they are written in. Nesting a copy inside each
 * payload puts it one level too deep and every entry point dies with
 * ERR_MODULE_NOT_FOUND at run time, long after the install reported success.
 */
function sharedLibDir(env) {
  return join(configRoot(env), 'hosts', 'lib')
}

/** Drops the shared engine once no host is left to import it. */
function pruneSharedLib(env) {
  if (Object.keys(hostsState(env).hosts).length > 0) return
  rmSync(sharedLibDir(env), { recursive: true, force: true })
}

function hostsState(env) {
  const path = join(configRoot(env), 'hosts.json')
  try {
    return { path, ...JSON.parse(readFileSync(path, 'utf8')) }
  } catch {
    return { path, hosts: {} }
  }
}

function saveHostsState(state, env) {
  mkdirSync(configRoot(env), { recursive: true })
  writeFileSync(state.path, `${JSON.stringify({ hosts: state.hosts }, null, 2)}\n`)
}

function readJson(path, fallback) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    return fallback
  }
}

/** Our hook entries are tagged so we can find and remove exactly them. */
function isOurs(entry) {
  return JSON.stringify(entry).includes(HOOK_TAG)
}

function writeSettings(claudeDir, mutate) {
  const path = join(claudeDir, 'settings.json')
  const settings = readJson(path, {})
  const next = mutate(settings)
  mkdirSync(claudeDir, { recursive: true })
  if (existsSync(path)) {
    writeFileSync(`${path}.consensflow.bak`, readFileSync(path))
  }
  writeFileSync(path, `${JSON.stringify(next, null, 2)}\n`)
}

function everyFile(dir) {
  const found = []
  for (const name of readdirSync(dir)) {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) found.push(...everyFile(path))
    else found.push(path)
  }
  return found
}

/** Where the payloads ship: inside this package, beside src/. */
function bundledRoot() {
  return join(dirname(fileURLToPath(import.meta.url)), '..', 'hosts')
}

const VERSION = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', 'package.json'), 'utf8'),
).version

function presence(id, env) {
  if (id === 'claude') {
    const cache = join(env.HOME ?? homedir(), '.claude', 'plugins', 'cache', 'consensflow-cc')
    return existsSync(cache) ? 'the Claude Code plugin marketplace' : null
  }
  if (id === 'pi') {
    const listed = spawnSync('pi', ['list'], { env, encoding: 'utf8' })
    return (listed.stdout ?? '').includes('consensflow-pi') ? 'pi install' : null
  }
  return null
}

function installClaude(env, options) {
  // Two ConsensFlow installs in one Claude Code means two skills with the
  // same name and, worse, hooks that fire twice per session.
  if (options.force !== true && presence('claude', env) !== null) {
    throw new Error(
      'ConsensFlow is already installed in Claude Code via the plugin marketplace — remove it there first (/plugin uninstall consensflow@consensflow-cc), or pass --force to install alongside it',
    )
  }
  const claudeDir = claudeConfigDir(env)
  const target = payloadDir(env)
  const bundled = options.bundled ?? bundledRoot()

  // The payload travels with the manager, so an install is a copy: no clone,
  // no network, and the payload always matches the manager that installed it.
  rmSync(target, { recursive: true, force: true })
  mkdirSync(target, { recursive: true })
  cpSync(join(bundled, 'claude'), target, { recursive: true })
  rmSync(sharedLibDir(env), { recursive: true, force: true })
  cpSync(join(bundled, 'lib'), sharedLibDir(env), { recursive: true })

  const written = []
  // Payload files address their own root symbolically; make it concrete.
  // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
  const rewrite = (text) => text.split('${CONSENSFLOW_HOST_ROOT}').join(target)

  const skillSource = join(target, 'skills', 'consensflow', 'SKILL.md')
  if (existsSync(skillSource)) {
    const skillPath = join(claudeDir, 'skills', 'consensflow', 'SKILL.md')
    mkdirSync(dirname(skillPath), { recursive: true })
    writeFileSync(skillPath, rewrite(readFileSync(skillSource, 'utf8')))
    written.push(skillPath)
  }

  const commandSource = join(target, 'commands', 'cf.md')
  if (existsSync(commandSource)) {
    const commandPath = join(claudeDir, 'commands', 'consensflow.md')
    mkdirSync(dirname(commandPath), { recursive: true })
    writeFileSync(commandPath, rewrite(readFileSync(commandSource, 'utf8')))
    written.push(commandPath)
  }

  const hookSource = existsSync(join(target, 'hooks', 'hooks.json'))
    ? join(target, 'hooks', 'hooks.json')
    : join(target, 'hooks.json')
  const hooks = readJson(hookSource, { hooks: {} }).hooks ?? {}
  writeSettings(claudeDir, (settings) => {
    const merged = { ...(settings.hooks ?? {}) }
    for (const [event, entries] of Object.entries(hooks)) {
      // Ours are replaced, never stacked; everyone else's are left alone.
      const theirs = (merged[event] ?? []).filter((entry) => !isOurs(entry))
      merged[event] = [...theirs, ...JSON.parse(rewrite(JSON.stringify(entries)))]
    }
    return { ...settings, hooks: merged }
  })

  const state = hostsState(env)
  state.hosts.claude = {
    version: VERSION,
    payload: target,
    files: written,
    hookEvents: Object.keys(hooks),
    installedAt: new Date().toISOString(),
  }
  saveHostsState(state, env)
  return { host: 'claude', version: VERSION, files: written, payload: target }
}

function uninstallClaude(env) {
  const state = hostsState(env)
  const record = state.hosts.claude
  const claudeDir = claudeConfigDir(env)

  for (const path of record?.files ?? []) {
    rmSync(path, { force: true })
    try {
      if (readdirSync(dirname(path)).length === 0) rmSync(dirname(path), { recursive: true })
    } catch {
      // Already gone, or someone else's files live there too.
    }
  }
  rmSync(record?.payload ?? payloadDir(env), { recursive: true, force: true })

  if (existsSync(join(claudeDir, 'settings.json'))) {
    writeSettings(claudeDir, (settings) => {
      const merged = { ...(settings.hooks ?? {}) }
      for (const event of Object.keys(merged)) {
        const theirs = merged[event].filter((entry) => !isOurs(entry))
        // An event that existed only to hold our hook goes with it; one that
        // still holds someone else's stays, minus ours.
        if (theirs.length === 0) delete merged[event]
        else merged[event] = theirs
      }
      return { ...settings, hooks: merged }
    })
  }

  delete state.hosts.claude
  saveHostsState(state, env)
  pruneSharedLib(env)
  return { host: 'claude', removed: record?.files?.length ?? 0 }
}

function pi(args, env) {
  const result = spawnSync('pi', args, { env, encoding: 'utf8' })
  if (result.error !== undefined) {
    throw new Error(
      `pi is not installed, so its extension cannot be managed (${result.error.message})`,
    )
  }
  if (result.status !== 0) {
    throw new Error(`pi ${args.join(' ')} failed: ${(result.stderr || result.stdout || '').trim()}`)
  }
  return result.stdout ?? ''
}

export function installHost(host, env, options = {}) {
  if (host === 'claude') return installClaude(env, options)
  if (host === 'pi') {
    // pi installs from a path (`pi install ./local/path`), so the bundled
    // payload is copied where it can live, then handed to pi's own CLI —
    // pi's settings stay pi's to write.
    const target = join(configRoot(env), 'hosts', 'pi')
    const bundled = options.bundled ?? bundledRoot()
    rmSync(target, { recursive: true, force: true })
    mkdirSync(target, { recursive: true })
    cpSync(join(bundled, 'pi'), target, { recursive: true })
    rmSync(sharedLibDir(env), { recursive: true, force: true })
    cpSync(join(bundled, 'lib'), sharedLibDir(env), { recursive: true })

    const source = options.source ?? target
    pi(['install', source], env)
    const state = hostsState(env)
    state.hosts.pi = { version: VERSION, source, installedAt: new Date().toISOString() }
    saveHostsState(state, env)
    return { host: 'pi', version: VERSION, source, ok: true }
  }
  throw new Error(`unknown host ${JSON.stringify(host)}; expected ${HOSTS.join(', ')}`)
}

export function uninstallHost(host, env, options = {}) {
  if (host === 'claude') return uninstallClaude(env)
  if (host === 'pi') {
    const state0 = hostsState(env)
    pi(['remove', options.source ?? state0.hosts.pi?.source ?? PI_SOURCE], env)
    rmSync(join(configRoot(env), 'hosts', 'pi'), { recursive: true, force: true })
    const state = hostsState(env)
    delete state.hosts.pi
    saveHostsState(state, env)
    pruneSharedLib(env)
    return { host: 'pi', ok: true }
  }
  throw new Error(`unknown host ${JSON.stringify(host)}; expected ${HOSTS.join(', ')}`)
}

/**
 * A host we installed ourselves is `installed` (with the commit we put
 * there). One installed another way — the Claude Code plugin marketplace,
 * `pi install` run by hand — is `present`: we did not put it there and will
 * not remove it, but saying "not installed" about a working integration
 * would be a lie.
 */
export function hostStatus(env) {
  const state = hostsState(env)
  return HOSTS.map((id) => {
    const record = state.hosts[id]
    const via = record !== undefined ? 'consensflow' : presence(id, env)
    return {
      id,
      installed: record !== undefined,
      present: via !== null,
      via,
      version: record?.version,
      source: record?.source,
      files: record?.files?.length ?? 0,
    }
  })
}

/** Files the claude payload occupies, for reporting. */
export function claudePayloadFiles(env) {
  const target = payloadDir(env)
  if (!existsSync(target)) return []
  return everyFile(target).map((path) => relative(target, path))
}
