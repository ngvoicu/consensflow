import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { readdirSync, readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { cliTarget } from './cli-target.mjs'
import { tempEnv } from './helpers.mjs'

/**
 * Every `cf …` that ConsensFlow's own words name is a command cf has: a
 * chief switched in was told to run `cf task show T-1`, which cf refuses
 * (2026-10-03). What cf has is read from its two usages, the window's and
 * the one outside a window, not from a list kept here. `cf hook <harness>`
 * is the one command neither names: the harnesses' hooks run it, never a
 * model.
 */
const REPO = fileURLToPath(new URL('..', import.meta.url))
/** The window's usage, the text the native cf prints. */
const USAGE = readFileSync(`${REPO}crates/cf/src/board/usage.txt`, 'utf8')

/** Each command and its verbs (null: it takes arguments, not a verb), from a usage's lines. */
function commandsOf(usage, prefix) {
  const commands = new Map()
  for (const line of usage.split('\n')) {
    const match = new RegExp(`^\\s+${prefix}([a-z]+)(?: \\[?([a-z|]+)\\b)?`).exec(line)
    if (match === null) continue
    const [, command, verbs] = match
    if (!commands.has(command)) commands.set(command, null)
    if (verbs !== undefined) {
      const known = commands.get(command) ?? new Set()
      for (const verb of verbs.split('|')) known.add(verb)
      commands.set(command, known)
    }
  }
  return commands
}

// Run here, not in a window: a window's token makes cf its board. By the cf these
// tests run: the native one, or Node's (tests/cli-target.mjs, `npm run test:clis`),
// in a home of its own.
const target = cliTarget()
const outside = (() => {
  const t = tempEnv()
  try {
    return execFileSync(target.command, [...target.args, 'help'], {
      cwd: REPO,
      encoding: 'utf8',
      env: { ...process.env, ...t.env, CONSENSFLOW_TOKEN: '' },
    })
  } finally {
    t.cleanup()
  }
})()
const COMMANDS = new Map([
  ...commandsOf(outside, ''),
  ...commandsOf(USAGE, 'cf '),
  ['help', null],
  ['hook', null],
])

/**
 * The files whose words reach a model or a person: code, role texts, the eval
 * prompts, the readme. Read from the folders: the gate's tree has no .git.
 */
const FILES = [
  'README.md',
  ...['src', 'hosts', 'skill', 'bin', 'app/ui', 'evals'].flatMap((root) =>
    readdirSync(`${REPO}${root}`, { recursive: true }).map((file) => `${root}/${file}`),
  ),
]
  .map((file) => file.replaceAll('\\', '/'))
  .filter(
    (file) =>
      /\.(js|mjs|md|html)$/.test(file) &&
      !file.includes('node_modules/') &&
      !file.startsWith('app/ui/vendor/') &&
      !file.startsWith('evals/reports/'),
  )

describe('the cf commands ConsensFlow names', () => {
  it('knows the window commands and the ones outside a window', () => {
    assert.deepEqual([...COMMANDS.get('task')].sort(), [
      'accept',
      'add',
      'cancel',
      'done',
      'get',
      'list',
      'pause',
      'reopen',
      'resume',
    ])
    for (const command of [
      'inbox',
      'ask',
      'answer',
      'tell',
      'history',
      'agent',
      'setup',
      'doctor',
    ]) {
      assert.ok(COMMANDS.has(command), command)
    }
  })

  it('names only commands and verbs cf has', () => {
    const wrong = []
    for (const file of FILES) {
      const text = readFileSync(`${REPO}${file}`, 'utf8')
      for (const match of text.matchAll(
        /(`?)(?<![\w./$-])cf ([a-z]+)(?: ([a-z]+(?:\|[a-z]+)*))?/g,
      )) {
        const [, quoted, command, verbs] = match
        const at = `${file}:${text.slice(0, match.index).split('\n').length}`
        if (!COMMANDS.has(command)) {
          // Prose may say "cf is the board"; in Markdown a quoted command
          // must be one (in code a backtick opens a template string).
          if (quoted && file.endsWith('.md')) wrong.push(`${at}: cf ${command}`)
          continue
        }
        const known = COMMANDS.get(command)
        if (known === null || verbs === undefined) continue
        for (const verb of verbs.split('|')) {
          if (!known.has(verb)) wrong.push(`${at}: cf ${command} ${verb}`)
        }
      }
    }
    assert.deepEqual(wrong, [])
  })
})
