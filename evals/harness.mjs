import { spawn } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

/**
 * A real lead, a real installed skill, and no real side effects.
 *
 * The point of an eval is that nothing here is simulated except the
 * consequences: the lead is the actual CLI, reading the actual generated
 * SKILL.md from the real home — which is the artefact under test. What IS
 * replaced is `cf` and `cmux`, by stubs that answer plausibly and record every
 * invocation. The lead's choices are then readable as a log, which is the only
 * honest way to ask "did the prose work".
 *
 * It runs in a throwaway directory so the lead has no project to touch, and
 * the stubs come first on PATH so nothing it types reaches a real pane or a
 * real agent.
 */

const CF_STUB = (log) => `#!/bin/sh
printf 'cf %s\\n' "$*" >> "${log}"
case "$1" in
  mint) echo "amber-tide" ;;
  sessions) echo "amber-tide        @nyx         0 runs   2026-08-24T16:00:00.000Z" ;;
  catchup)
    case "$*" in
      *--unread*) echo "amber-tide · @nyx · 2 new turns"; echo ""; echo "› asked"; echo "do you have more?"; echo ""; echo "• @nyx"; echo "Why do Java developers wear glasses? Because they can't C#." ;;
      *) echo "amber-tide · @nyx · 4 turns"; echo ""; echo "› asked"; echo "Tell me a joke"; echo ""; echo "• @nyx"; echo "Light attracts bugs."; echo ""; echo "› asked"; echo "do you have more?"; echo ""; echo "• @nyx"; echo "Why do Java developers wear glasses? Because they can't C#." ;;
    esac ;;
  run) echo "conversation: amber-tide (new)"; echo "read it back with: cf catchup amber-tide"; echo "Light attracts bugs."; echo "— @nyx" ;;
  last) echo "# amber-tide · @nyx"; echo ""; echo "Light attracts bugs." ;;
  *) : ;;
esac
`

const CMUX_STUB = (log) => `#!/bin/sh
printf 'cmux %s\\n' "$*" >> "${log}"
case "$1" in
  new-pane) echo "OK surface:99 pane:99 workspace:1" ;;
  send) echo "OK surface:99 workspace:1" ;;
  rename-tab) echo "OK action=rename tab=tab:99 workspace=workspace:1" ;;
  tree) echo "window window:1 [current]"; echo "\\_ workspace workspace:1"; echo "   |- pane pane:28"; echo "   |   \\_ surface surface:28 [terminal] \\"the lead\\" [selected] <- here"; echo "   \\_ pane pane:99"; echo "       \\_ surface surface:99 [terminal] \\"amber-tide\\" [selected]" ;;
  # The real one lists ONLY the caller's own pane, whatever you pass it
  # (probed live, two panes open, 2026-08-26). A stub that helpfully listed
  # both would let a lead pass the eval with a command that finds nothing.
  list-pane-surfaces) echo "* surface:28  the lead  [selected]" ;;
  list-panes) echo "* pane:28  [1 surface]  [focused]"; echo "  pane:99  [1 surface]" ;;
  *) : ;;
esac
`

export function makeStage() {
  const root = mkdtempSync(join(tmpdir(), 'cf-eval-'))
  const bin = join(root, 'bin')
  const cwd = join(root, 'work')
  const log = join(root, 'commands.log')
  mkdirSync(bin, { recursive: true })
  mkdirSync(cwd, { recursive: true })
  writeFileSync(log, '')
  for (const [name, body] of [
    ['cf', CF_STUB(log)],
    ['cmux', CMUX_STUB(log)],
  ]) {
    const path = join(bin, name)
    writeFileSync(path, body)
    chmodSync(path, 0o755)
  }
  return {
    cwd,
    log,
    // The stubs come first; everything else the lead needs stays reachable.
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, CMUX_QUIET: '1' },
    read: () => readFileSync(log, 'utf8').split('\n').filter(Boolean),
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  }
}

/**
 * One lead session, resumed across turns — the failures all happened on turn
 * two or later, so a scenario that could only ask once would miss every one.
 */
export function leadSession(kind) {
  if (kind === 'claude') {
    const id = randomUUID()
    let started = false
    return (say) => {
      const args = started ? ['--resume', id] : ['--session-id', id]
      started = true
      return { command: 'claude', args: [...args, '-p', say, '--dangerously-skip-permissions'] }
    }
  }
  if (kind === 'codex') {
    let thread = null
    return (say, previous) => {
      if (previous) thread = previous
      const base = thread ? ['exec', 'resume', thread] : ['exec']
      return {
        command: 'codex',
        args: [
          ...base,
          '--json',
          '--skip-git-repo-check',
          '--dangerously-bypass-approvals-and-sandbox',
          say,
        ],
        capturesThread: true,
      }
    }
  }
  throw new Error(`no eval lead for ${kind} — claude and codex are supported`)
}

export function runLead({ command, args }, { cwd, env }, timeoutMs) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] })
    let stdout = ''
    let stderr = ''
    child.stdout.on('data', (chunk) => {
      stdout += chunk
    })
    child.stderr.on('data', (chunk) => {
      stderr += chunk
    })
    const timer = setTimeout(() => child.kill('SIGKILL'), timeoutMs)
    child.on('close', (code) => {
      clearTimeout(timer)
      resolve({ code, stdout, stderr })
    })
    child.on('error', (cause) => {
      clearTimeout(timer)
      resolve({ code: 127, stdout, stderr: String(cause) })
    })
  })
}

/** codex hands its thread id back on the stream; claude was given one. */
export function threadFrom(stdout) {
  for (const line of stdout.split('\n')) {
    try {
      const parsed = JSON.parse(line)
      if (parsed?.thread_id) return parsed.thread_id
    } catch {
      // not every line is json
    }
  }
  return null
}
