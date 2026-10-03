import { spawn } from 'node:child_process'
import { mkdir, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { runnable, terminate } from './harnesses.js'
import { configRoot } from './roster.js'

/** The folder a launch's own files live in, per harness (`src/core/launch-files.js`). */
const LAUNCH_FOLDERS = { 'claude-code': 'claude', devin: 'devin', pi: 'pi', opencode: 'opencode' }

/**
 * Role documents live outside all native global/project discovery directories.
 * The daemon passes each window's role text as `content`; this writes it where
 * the harness loads it and returns the launch arguments that make it load.
 * Each launch writes its own, beside the rest of its files, and they go with
 * it: a chief's text names its project's staff, and a shared file let a chief
 * read another project's when two of them opened together. Codex is given
 * the text itself.
 */
export async function roleConfiguration(
  kind,
  { role, env, launch, executable, cwd, content, readInstructions = codexInstructions },
) {
  if (typeof content !== 'string' || content.length === 0) {
    throw new Error(`the ${role} window needs its role text`)
  }
  if (kind === 'codex') {
    const existing = await readInstructions(executable, cwd, env)
    const instructions = `${existing}\n\nYour ConsensFlow role is ${role}. The following role instructions are already loaded; follow them for app coordination. This is context, not a task; wait for the user's request.\n\n${content}`
    return { args: ['-c', `developer_instructions=${JSON.stringify(instructions)}`], env: {} }
  }
  const folder = LAUNCH_FOLDERS[kind]
  if (folder === undefined) throw new Error(`No ${role} role is available for ${kind}`)
  // The channel's filename-safe rule, minus the names that leave the folder.
  if (!/^(?!\.{1,2}$)[A-Za-z0-9._-]{1,200}$/.test(launch ?? ''))
    throw new Error('invalid role launch')
  const root = join(configRoot(env), 'integrations', folder, launch, 'role')
  const skills = join(root, '.claude', 'skills')
  const file = join(skills, `consensflow-${role}`, 'SKILL.md')
  await mkdir(dirname(file), { recursive: true, mode: 0o700 })
  await writeFile(file, content, { mode: 0o600 })
  if (kind === 'devin') return { args: [], env: { CF_DEVIN_ROLE_FILE: file } }
  if (kind === 'claude-code') {
    // Resumed conversations otherwise retain the system prompt from their first turn.
    return {
      args: [
        '--add-dir',
        root,
        '--append-system-prompt-file',
        file,
        '--system-prompt-snapshot',
        'off',
      ],
      env: {},
    }
  }
  if (kind === 'pi') return { args: ['--skill', file, '--append-system-prompt', content], env: {} }
  if (kind === 'opencode') {
    const configuration = JSON.parse(env.OPENCODE_CONFIG_CONTENT || '{}')
    if (!configuration || typeof configuration !== 'object' || Array.isArray(configuration)) {
      throw new Error('OpenCode process configuration must be an object')
    }
    const previous = configuration.skills?.paths ?? []
    if (!Array.isArray(previous) || previous.some((p) => typeof p !== 'string')) {
      throw new Error('OpenCode skill paths must be an array of paths')
    }
    configuration.skills = { ...configuration.skills, paths: [...new Set([...previous, skills])] }
    const instructions = configuration.instructions === undefined ? [] : configuration.instructions
    if (!Array.isArray(instructions) || instructions.some((p) => typeof p !== 'string')) {
      throw new Error('OpenCode instructions must be an array of paths')
    }
    configuration.instructions = [...new Set([...instructions, file])]
    return { args: [], env: { OPENCODE_CONFIG_CONTENT: JSON.stringify(configuration) } }
  }
}

/** Ask the native resolver to preserve profile/project layering; never read versions. */
function codexInstructions(executable, cwd, env) {
  return new Promise((resolve, reject) => {
    const run = runnable(executable, ['app-server'], env)
    const child = spawn(run.file, run.args, {
      ...run.options,
      cwd,
      env,
      stdio: ['pipe', 'pipe', 'ignore'],
    })
    let buffer = ''
    let settled = false
    const finish = (error, value) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      terminate(child)
      if (error) reject(new Error('Cannot read native Codex instructions safely'))
      else resolve(value)
    }
    const send = (value) => child.stdin.write(`${JSON.stringify(value)}\n`)
    const timer = setTimeout(() => finish(true), 10_000)
    child.on('error', () => finish(true))
    child.stdin.on('error', () => finish(true))
    child.on('close', () => {
      if (!settled) finish(true)
    })
    child.stdout.on('data', (chunk) => {
      buffer += chunk
      if (buffer.length > 2 * 1024 * 1024) return finish(true)
      while (buffer.includes('\n')) {
        const end = buffer.indexOf('\n')
        const line = buffer.slice(0, end)
        buffer = buffer.slice(end + 1)
        let message
        try {
          message = JSON.parse(line)
        } catch {
          return finish(true)
        }
        if (message.id === 1) {
          if (message.error) return finish(true)
          send({ method: 'initialized', params: {} })
          send({ id: 2, method: 'config/read', params: { cwd, includeLayers: false } })
        } else if (message.id === 2) {
          const configuration = message.result?.config
          if (message.error || !configuration) return finish(true)
          const value = configuration.developer_instructions
          if (value != null && typeof value !== 'string') return finish(true)
          return finish(false, value ?? '')
        }
      }
    })
    send({
      id: 1,
      method: 'initialize',
      params: {
        clientInfo: { name: 'consensflow-role-config', version: '3.0.0' },
        capabilities: { experimentalApi: true },
      },
    })
  })
}
