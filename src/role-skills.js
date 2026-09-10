import { spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { configRoot, listAgents } from './roster.js'
import { generateSkill } from './skill.js'

/** Role documents live outside all native global/project discovery directories. */
export async function roleConfiguration(
  kind,
  { role, env, executable, cwd, readInstructions = codexInstructions },
) {
  if (role !== 'lead' && role !== 'pm') return { args: [], env: {} }
  const name = `consensflow-${role}`
  const root = join(configRoot(env), 'roles', role)
  const skills = join(root, '.claude', 'skills')
  const directory = join(skills, name)
  const file = join(directory, 'SKILL.md')
  const content =
    role === 'lead'
      ? generateSkill(listAgents(env))
      : await readFile(new URL(`../skill/roles/${name}/SKILL.md`, import.meta.url), 'utf8')
  await mkdir(directory, { recursive: true, mode: 0o700 })
  await writeFile(file, content, { mode: 0o600 })
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
  if (kind === 'codex') {
    const existing = await readInstructions(executable, cwd, env)
    const instructions = `${existing}\n\nYour ConsensFlow role is ${role}. The following role instructions are already loaded; follow them for app coordination. This is context, not a task; wait for the user's request.\n\n${content}`
    return { args: ['-c', `developer_instructions=${JSON.stringify(instructions)}`], env: {} }
  }
  throw new Error(`No ${role} role is available for ${kind}`)
}

/** Ask the native resolver to preserve profile/project layering; never read versions. */
function codexInstructions(executable, cwd, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, ['app-server'], { cwd, env, stdio: ['pipe', 'pipe', 'ignore'] })
    let buffer = ''
    let settled = false
    const finish = (error, value) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      child.kill()
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
