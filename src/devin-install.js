import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import { dirname, isAbsolute, join } from 'node:path'
import { probeExecutable } from './harnesses.js'
import { preparePrivateIntegration } from './private-integration.js'
import { configRoot } from './roster.js'

const FILES = ['hosts/devin-hooks.mjs']
const quote = (value) => `'${String(value).replaceAll("'", "'\\''")}'`

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
  const file = join(
    env.XDG_CONFIG_HOME ?? join(env.HOME ?? homedir(), '.config'),
    'devin',
    'config.json',
  )
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
export async function prepareDevinIntegration(
  env,
  { launchId, node, executable, boardQuestions = true },
) {
  if (!/^[A-Za-z0-9_-]{1,200}$/.test(launchId ?? '')) throw new Error('invalid Devin launch')
  if (typeof node !== 'string' || !isAbsolute(node))
    throw new Error('Devin requires an absolute runtime')
  if (executable) {
    // Asked once per executable as it is on disk, not at every launch.
    const { stdout } = await probeExecutable(executable, ['--version'], env)
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
  // A session the window shows starts with its role text.
  const starts = configuration.hooks.SessionStart ?? []
  if (!Array.isArray(starts)) throw new Error('Invalid native Devin hook configuration')
  configuration.hooks.SessionStart = [
    ...starts,
    { matcher: '', hooks: [{ type: 'command', command, timeout: 5 }] },
  ]
  // A member's question tool, answered from the board: the hook holds the call
  // while the question waits for its answer (`cf hook devin`). The chief's
  // shows Devin's own dialog, where the human answers it.
  const questions = configuration.hooks.PreToolUse ?? []
  if (!Array.isArray(questions)) throw new Error('Invalid native Devin hook configuration')
  configuration.hooks.PreToolUse = [
    ...questions,
    ...(boardQuestions
      ? [
          {
            matcher: 'ask_user_question',
            hooks: [{ type: 'command', command: 'cf hook devin', timeout: QUESTION_HOOK_SECONDS }],
          },
        ]
      : []),
  ]
  configuration.auto_update = false
  const file = join(root, 'config.json')
  await writeFile(file, JSON.stringify(configuration), { mode: 0o600, flag: 'wx' })
  return {
    args: ['--config', file],
    env: { CHISEL_PURE_ACP_WIRE_LOG: join(root, 'wire.jsonl') },
    channel: { kind: 'devin-tui', launchId, wire: join(root, 'wire.jsonl') },
  }
}

export async function prepareDevinPrompt(invocation, configuration) {
  if (invocation.prompt === undefined) return invocation
  // In the launch's own folder, beside its wire log.
  const file = join(dirname(configuration.channel.wire), 'prompt.txt')
  await writeFile(file, invocation.prompt, { mode: 0o600, flag: 'wx' })
  const { prompt: _prompt, ...native } = invocation
  return { ...native, args: [...native.args, '--prompt-file', file] }
}
