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
import { terminalCommandStatus } from './terminal.js'

/**
 * Host integrations: the deeper ConsensFlow paths that live inside a coding
 * harness and can hand it your live conversation.
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

/** Every file of an installed payload, so a placeholder cannot hide in one. */
/** The spelling of the CLI this machine should be told to run. */
function cliCommand(env, target) {
  return terminalCommandStatus(env).onPath
    ? 'cf'
    : `"${process.execPath}" "${join(target, 'bin', 'cf.mjs')}"`
}

function payloadFiles(root) {
  const found = []
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name)
      if (entry.isDirectory()) walk(full)
      else if (/\.(mjs|js|ts|md|json)$/.test(entry.name)) found.push(full)
    }
  }
  try {
    walk(root)
  } catch {
    // Nothing installed yet; nothing to rewrite.
  }
  return found
}

/**
 * The same substitution, applied to a parsed value rather than to JSON text.
 *
 * Rewriting the serialized form breaks on Windows: `C:\\Users\\…\\node.exe`
 * dropped into a JSON string leaves `\\U`, which is not a valid escape, and the
 * hooks file stops parsing. Walking the structure and letting JSON.stringify
 * escape at write time cannot get that wrong.
 */
function rewriteDeep(value, rewrite) {
  if (typeof value === 'string') return rewrite(value)
  if (Array.isArray(value)) return value.map((item) => rewriteDeep(item, rewrite))
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, rewriteDeep(v, rewrite)]))
  }
  return value
}

/**
 * Takes back the SessionStart / UserPromptSubmit entries ConsensFlow used to
 * install. Nothing writes them any more; this exists so upgrading removes the
 * old design rather than leaving it running against deleted scripts.
 */
function removeOurHooks(claudeDir) {
  if (!existsSync(join(claudeDir, 'settings.json'))) return
  writeSettings(claudeDir, (settings) => {
    const merged = { ...(settings.hooks ?? {}) }
    for (const event of Object.keys(merged)) {
      const theirs = merged[event].filter((entry) => !isOurs(entry))
      if (theirs.length === 0) delete merged[event]
      else merged[event] = theirs
    }
    return { ...settings, hooks: merged }
  })
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
  const rewrite = (text) =>
    text
      // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
      .split('${CONSENSFLOW_HOST_ROOT}')
      .join(target)
      // What the lead types. `cf` when the launcher is on PATH — the same line
      // every skill teaches — and this payload's own CLI when it is not, so a
      // machine whose PATH lacks the launcher directory still works. Quoted
      // occurrences are replaced as a whole, so the value lands as a valid JS
      // string rather than nesting quotes inside one.
      // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
      .split('"${CONSENSFLOW_CLI}"')
      .join(JSON.stringify(cliCommand(env, target)))
      // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
      .split('${CONSENSFLOW_CLI}')
      .join(cliCommand(env, target))
      // The runtime, named absolutely. A bare `node` in a hook is a bet that
      // the machine has Node on PATH — and the app exists precisely so that
      // bet is never needed: it carries its own, and that is the one running
      // this install.
      // biome-ignore lint/suspicious/noTemplateCurlyInString: the payload's own placeholder, not a template
      .split('${CONSENSFLOW_NODE}')
      .join(process.execPath)

  // The payload is copied verbatim, so its own files still carry the
  // placeholders. Rewrite them where they landed: a script that names the CLI
  // symbolically is useless if the symbol survives the install.
  for (const file of payloadFiles(target)) {
    const before = readFileSync(file, 'utf8')
    if (!before.includes('${CONSENSFLOW_')) continue
    if (file.endsWith('.json')) {
      // Structurally, so a Windows path lands as data rather than as escapes.
      writeFileSync(file, `${JSON.stringify(rewriteDeep(JSON.parse(before), rewrite), null, 2)}\n`)
    } else {
      writeFileSync(file, rewrite(before))
    }
  }

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

  // No hooks. They existed to stash the session for the handoff, and that
  // is now the lead's to pass with --handoff-file — the same in every mode.
  // A machine that has ours from an earlier version gets them taken away
  // below, so an upgrade cleans up after the old design.
  removeOurHooks(claudeDir)

  const state = hostsState(env)
  state.hosts.claude = {
    version: VERSION,
    payload: target,
    files: written,
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

  removeOurHooks(claudeDir)

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
/**
 * The runtime an installed host payload was wired to.
 *
 * Hooks name it absolutely, so a machine with no Node still works — but the
 * flip side is that moving or deleting whatever provided it (the app, usually)
 * leaves the hooks pointing at nothing. Reporting that is cheaper than letting
 * every prompt fail quietly.
 */
export function hostRuntime(env) {
  const record = hostsState(env).hosts.claude
  if (record === undefined) return null
  // The `/consensflow` command is the one installed file that still names a
  // runtime — the hooks that used to are gone. If whatever provided it moved,
  // the command breaks, and doctor should say so rather than let it fail.
  const commandFile = join(claudeConfigDir(env), 'commands', 'consensflow.md')
  if (!existsSync(commandFile)) return null
  const runtime = readFileSync(commandFile, 'utf8').match(/"([^"]+)"\s+"[^"]*cf\.mjs"/)?.[1]
  if (runtime === undefined) return null
  return { runtime, exists: existsSync(runtime) }
}

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
