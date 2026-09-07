import { spawn } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

/**
 * The agent every scenario consults. It has to be a name the INSTALLED
 * skill's roster carries: the lead reads that roster before it consults, and
 * a name that is not there is refused, not tried — `nyx` was refused by a
 * lead on 2026-09-06 ("no agent named nyx") and every check read as 0/3
 * while the prose was fine. The stub `cf` answers for any name; the lead
 * is the one that has to believe it. `run.mjs` checks the roster at start.
 */
export const AGENT = process.env.CF_EVAL_AGENT ?? 'zeus'

/**
 * A real lead, a real installed skill, and no real side effects.
 *
 * The point of an eval is that nothing here is simulated except the
 * consequences: the lead is the actual CLI, reading the actual generated
 * SKILL.md from the real home — which is the artefact under test. What IS
 * replaced is `cf`, by a stub that answers plausibly and records every
 * invocation. The lead's choices are then readable as a log, which is the only
 * honest way to ask "did the prose work".
 *
 * It runs in a throwaway directory so the lead has no project to touch, and
 * the stub comes first on PATH so nothing it types reaches a real agent.
 * There is no cmux stub any more: ConsensFlow has one shape, the app owns
 * the panes, and the skill never names a pane tool.
 */

/**
 * A delivered file arrives in numbered parts, each closed by an end marker,
 * with the next part named. The lead has to run every part: a report built
 * from part 1 alone misses the middle and the end, and the scenario checks
 * for tokens from each one.
 */
const READ_CASES = (readParts) =>
  Object.entries(readParts ?? {})
    .map(([id, parts]) =>
      parts
        .map(
          (body, i) => `    ${id},$((${i} + 1))) printf '%s\\n' "${body}"${i + 1 < parts.length ? `; echo "next: cf read ${id} --part ${i + 2}"` : ''} ;;`,
        )
        .join('\n'),
    )
    .join('\n')

const CF_STUB = (log, { transcriptPath = null, deliverEnvelope = null, readParts = null } = {}) => `#!/bin/sh
printf 'cf %s\\n' "$*" >> "${log}"
case "$1" in
  # The app mints the name and prints it. The log already holds this call,
  # so a second --new is a second conversation — which is what the
  # independent-task scenario asks for.
  run)
    name=""; fresh=""; prev=""
    for a in "$@"; do
      if [ "$prev" = "--session" ]; then name="$a"; fi
      if [ "$a" = "--new" ]; then fresh="1"; fi
      prev="$a"
    done
    if [ -n "$fresh" ]; then
      if [ "$(grep -c '^cf run.*--new' "${log}")" -ge 2 ]; then name="${AGENT}-coral-lane"; else name="amber-tide"; fi
      echo "conversation: $name (new)"
    elif [ -z "$name" ]; then
      name="amber-tide"; echo "conversation: $name (continued)"
    else
      echo "conversation: $name (continued)"
    fi
    echo "discover results with: cf results $name" ;;
  say) echo "said into $2" ;;
  results)
    echo "amber-tide · @${AGENT} — 1 completed result"
    echo "result-1 · unread — read with: cf read amber-tide" ;;
  # A delivery already visible in the lead transcript is reported directly.
  deliver)
    echo "delivered $2"
    ${transcriptPath && deliverEnvelope ? `printf '%s\\n' "${deliverEnvelope}" >> "${transcriptPath}"` : ':'} ;;
  read)
    case "$2" in
      d-*) ;;
      *) ${transcriptPath ? `cat "${transcriptPath}"` : `echo "Why do Java developers wear glasses? Because they can't C#."`}; exit 0 ;;
    esac
    part="1"; if [ "$3" = "--part" ]; then part="$4"; fi
    case "$2,$part" in
${READ_CASES(readParts) || '      *,*) echo "no such delivery part" ;;'}
      *,*) echo "no such delivery part" ;;
    esac ;;
  *) : ;;
esac
`

export function makeStage(options = {}) {
  const root = mkdtempSync(join(tmpdir(), 'cf-eval-'))
  const bin = join(root, 'bin')
  const cwd = join(root, 'work')
  const log = join(root, 'commands.log')
  mkdirSync(bin, { recursive: true })
  mkdirSync(cwd, { recursive: true })
  writeFileSync(log, '')
  // A scenario whose prompt names a file needs that file to be there. Without
  // it the lead reasonably checks, finds nothing, and asks the user instead of
  // consulting — which reads in the tally as "never consulted" and blames
  // the skill for the scenario's own missing prop.
  for (const [rel, body] of Object.entries(options.files ?? {})) {
    const path = join(cwd, rel)
    mkdirSync(dirname(path), { recursive: true })
    writeFileSync(path, body)
  }
  // A scenario whose follow-up refers to what the agent SAID needs the agent
  // to have said it. The default stub answers with jokes whatever was asked,
  // so "a test for the case he flagged" met a conversation that flagged
  // nothing — and a lead that looks before it sends, as the rules say, then
  // correctly finds nothing to send. Scenario props, not skill failures.
  let transcriptPath = null
  if (options.transcript) {
    transcriptPath = join(root, 'transcript.txt')
    writeFileSync(transcriptPath, options.transcript)
  }
  const cfPath = join(bin, 'cf')
  writeFileSync(cfPath, CF_STUB(log, { ...options, transcriptPath }))
  chmodSync(cfPath, 0o755)
  return {
    cwd,
    log,
    // The stub comes first; everything else the lead needs stays reachable.
    env: { ...process.env, PATH: `${bin}:${process.env.PATH}` },
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
