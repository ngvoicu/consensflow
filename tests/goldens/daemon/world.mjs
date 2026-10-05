/**
 * The files and variables of the environment a daemon surface reads: the
 * roster in `CONSENSFLOW_HOME`, the harnesses' stand-ins on `PATH`. A player
 * puts them in place before the step that reads them, so a world is written
 * down whole at the first step of a trace and then as what changed.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, delimiter, join, relative, sep } from 'node:path'
import { maskStamps } from './mask.mjs'

const LIMIT = 1024 * 1024

/** The folder a test made that `path` lies in: the first one under the temporary folder, or null. */
export function treeOf(path) {
  const base = `${tmpdir()}${sep}`
  return typeof path === 'string' && path.startsWith(base)
    ? join(base, path.slice(base.length).split(sep)[0])
    : null
}

/**
 * The folder the test made that `env` names, when its values lie in one. A
 * test's whole tree is in there.
 */
export function rootOf(env) {
  const roots = new Set()
  for (const text of Object.values(env)) {
    if (typeof text !== 'string') continue
    for (const part of text.split(delimiter)) {
      const root = treeOf(part)
      if (root !== null) roots.add(root)
    }
  }
  if (roots.size > 1) throw new Error(`one folder of files per test: ${[...roots].join(', ')}`)
  return roots.values().next().value ?? null
}

/** A file as written down: its text, or its bytes when it is no UTF-8; a script's executable bit. */
function entry(path) {
  const bytes = readFileSync(path)
  const executable = process.platform !== 'win32' && (statSync(path).mode & 0o111) !== 0
  const read = bytes.toString('utf8')
  const same = Buffer.from(read, 'utf8').equals(bytes)
  const body = same
    ? { text: basename(path) === 'agents.json' ? maskStamps(read) : read }
    : { base64: bytes.toString('base64') }
  return { ...body, ...(executable ? { executable: true } : {}) }
}

function walk(folder, skip, into, root) {
  let names
  try {
    names = readdirSync(folder, { withFileTypes: true })
  } catch {
    return
  }
  for (const item of names.sort((a, b) => (a.name < b.name ? -1 : 1))) {
    const path = join(folder, item.name)
    if (skip.has(path)) continue
    if (item.isDirectory()) walk(path, skip, into, root)
    else if (item.isFile()) {
      if (statSync(path).size > LIMIT) throw new Error(`${path} is over ${LIMIT} bytes`)
      into.set(relative(root, path).split(sep).join('/'), entry(path))
    }
  }
}

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b)

/** The world as it is now: the variables `env` holds, and every file under `root` but the ledgers'. */
export function snapshot(env, root, ledgers) {
  const skip = new Set(
    ledgers.flatMap(({ record }) => [record.file, `${record.file}-wal`, `${record.file}-shm`]),
  )
  const files = new Map()
  walk(root, skip, files, root)
  const variables = Object.fromEntries(
    Object.entries(env).filter(([, text]) => typeof text === 'string'),
  )
  return {
    env,
    root,
    files,
    variables,
    full: () => ({ env: variables, files: Object.fromEntries(files) }),
    /** What changed since `before`: a variable or file that went is null. */
    since(before) {
      const env = {}
      for (const name of new Set([...Object.keys(before.variables), ...Object.keys(variables)])) {
        if (before.variables[name] !== variables[name]) env[name] = variables[name] ?? null
      }
      const changed = {}
      for (const name of new Set([...before.files.keys(), ...files.keys()])) {
        if (!same(before.files.get(name), files.get(name))) changed[name] = files.get(name) ?? null
      }
      const none = (object) => Object.keys(object).length === 0
      if (none(env) && none(changed)) return null
      return { ...(none(env) ? {} : { env }), ...(none(changed) ? {} : { files: changed }) }
    },
  }
}

/** The files two worlds differ in, each as it was and as it is (null: not there); null when none. */
export function wrote(before, after) {
  const changed = {}
  for (const name of new Set([...before.files.keys(), ...after.files.keys()])) {
    if (!same(before.files.get(name), after.files.get(name))) {
      changed[name] = {
        before: before.files.get(name) ?? null,
        after: after.files.get(name) ?? null,
      }
    }
  }
  return Object.keys(changed).length === 0 ? null : changed
}
