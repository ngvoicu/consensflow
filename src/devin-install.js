import { execFile } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, isAbsolute, join } from 'node:path'
import { promisify } from 'node:util'
import { runnable } from './harnesses.js'
import { preparePrivateIntegration } from './private-integration.js'
import { configRoot } from './roster.js'

const FILES = ['hosts/devin-receiver.mjs', 'hosts/lib/receiver.js', 'package.json']
const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`
const execute = promisify(execFile)

export const DEVIN_MINIMUM_VERSION = '3000.10.21'
/** How long Devin lets the question hook wait for the board's answer. */
const QUESTION_HOOK_SECONDS = 3600
export function supportedDevinVersion(value) {
  const match = String(value).match(/\b(\d+)\.(\d+)\.(\d+)\b/)
  if (!match) return false
  const parts = match.slice(1).map(Number),
    minimum = [3000, 10, 21]
  for (let i = 0; i < 3; i++) if (parts[i] !== minimum[i]) return parts[i] > minimum[i]
  return true
}

async function nativeConfiguration(env) {
  const file = join(env.XDG_CONFIG_HOME ?? join(env.HOME, '.config'), 'devin', 'config.json')
  let source
  try {
    source = await readFile(file, 'utf8')
  } catch (error) {
    if (error.code === 'ENOENT') return {}
    throw error
  }
  try {
    // Native JSONC supports comments and trailing commas. Quoted strings are
    // consumed as whole tokens so URLs, escapes and comment-like text survive.
    const stripped = source
      .replace(
        /("(?:\\[\s\S]|[^"\\])*")|\/\/[^\r\n]*|\/\*[\s\S]*?\*\//g,
        (_all, string) => string ?? ' ',
      )
      .replace(/("(?:\\[\s\S]|[^"\\])*")|,\s*(?=[}\]])/g, (_all, string) => string ?? '')
    const value = JSON.parse(stripped)
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error()
    if (value.hooks && (typeof value.hooks !== 'object' || Array.isArray(value.hooks)))
      throw new Error()
    return value
  } catch {
    throw new Error('Cannot read native Devin configuration; the original was preserved')
  }
}

/** Mutable native preferences are per launch; loaded helper code is immutable. */
export async function prepareDevinIntegration(env, { launchId, node, executable }) {
  if (!/^[A-Za-z0-9_-]{1,200}$/.test(launchId ?? '')) throw new Error('invalid Devin launch')
  if (typeof node !== 'string' || !isAbsolute(node))
    throw new Error('Devin requires an absolute runtime')
  if (executable) {
    const run = runnable(executable, ['--version'], env)
    const { stdout } = await execute(run.file, run.args, {
      ...run.options,
      env,
      timeout: 3000,
      maxBuffer: 8192,
    })
    if (!supportedDevinVersion(stdout))
      throw new Error(
        `Devin ${DEVIN_MINIMUM_VERSION} or newer is required for complete worker replies. Update Devin before opening this pane.`,
      )
  }
  const configuration = await nativeConfiguration(env)
  const destination = preparePrivateIntegration(env, 'devin', FILES)
  const root = join(configRoot(env), 'integrations', 'devin', launchId)
  await mkdir(root, { recursive: true, mode: 0o700 })
  const command = `${quote(node)} ${quote(join(destination, FILES[0]))}`
  configuration.hooks ??= {}
  for (const name of ['SessionStart', 'UserPromptSubmit', 'Stop', 'SessionEnd']) {
    const previous = configuration.hooks[name] ?? []
    if (!Array.isArray(previous)) throw new Error('Invalid native Devin hook configuration')
    configuration.hooks[name] = [
      ...previous,
      { matcher: '', hooks: [{ type: 'command', command, timeout: 5 }] },
    ]
  }
  // Devin's question tool, answered from the board: the hook holds the call
  // while the question waits for its answer (`cf hook devin`).
  const questions = configuration.hooks.PreToolUse ?? []
  if (!Array.isArray(questions)) throw new Error('Invalid native Devin hook configuration')
  configuration.hooks.PreToolUse = [
    ...questions,
    {
      matcher: 'ask_user_question',
      hooks: [{ type: 'command', command: 'cf hook devin', timeout: QUESTION_HOOK_SECONDS }],
    },
  ]
  configuration.auto_update = false
  const file = join(root, 'config.json')
  await writeFile(file, JSON.stringify(configuration), { mode: 0o600, flag: 'wx' })
  return {
    args: ['--config', file],
    env: {
      CHISEL_PURE_ACP_WIRE_LOG: join(root, 'wire.jsonl'),
      CF_DEVIN_EVENTS: join(root, 'hooks.jsonl'),
    },
    channel: { kind: 'devin-tui', launchId, wire: join(root, 'wire.jsonl') },
  }
}

export async function prepareDevinPrompt(invocation, configuration) {
  if (invocation.prompt === undefined) return invocation
  const file = join(dirname(configuration.env.CF_DEVIN_EVENTS), 'prompt.txt')
  await writeFile(file, invocation.prompt, { mode: 0o600, flag: 'wx' })
  const { prompt: _prompt, ...native } = invocation
  return { ...native, args: [...native.args, '--prompt-file', file] }
}
