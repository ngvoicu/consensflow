import { execFileSync } from 'node:child_process'

/**
 * What the process table says, which is the evidence the smoke has of the app,
 * of its daemon and of what outlives either. Only `ps` is asked, and only of
 * processes the smoke started: the app's, its daemon's and the windows' stand-ins.
 */

export const TIMEOUT_MS = Number(process.env.CONSENSFLOW_UPDATER_SMOKE_TIMEOUT_MS ?? 180_000)

/** The rows of `ps -axo pid=,ppid=,stat=,command=`. */
export function parseTable(text) {
  return text.split('\n').flatMap((line) => {
    const found = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(.*)$/.exec(line)
    return found === null
      ? []
      : [{ pid: Number(found[1]), ppid: Number(found[2]), state: found[3], command: found[4] }]
  })
}

export function processTable() {
  return parseTable(
    execFileSync('/bin/ps', ['-axo', 'pid=,ppid=,stat=,command='], {
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    }),
  )
}

/** The pids under `root` in `table`: its children, theirs, and so on. */
export function descendants(table, root) {
  const found = []
  const queue = [root]
  while (queue.length > 0) {
    const parent = queue.shift()
    for (const row of table) {
      if (row.ppid === parent && !found.includes(row.pid)) {
        found.push(row.pid)
        queue.push(row.pid)
      }
    }
  }
  return found
}

/**
 * Ends `root` and everything under it, round after round until nothing is left.
 * A process group is not enough where a chain of processes keeps making the next
 * (two programs handing a command to each other for ever did, once): what is
 * under the app is found by the table, and ended before it makes another.
 */
export function killTree(root) {
  for (let round = 0; round < 100; round += 1) {
    const under = descendants(processTable(), root)
    if (under.length === 0 && !alive(root)) return
    for (const pid of [root, ...under]) {
      try {
        process.kill(pid, 'SIGKILL')
      } catch {
        // Already gone.
      }
    }
  }
}

/** Whether the process runs: signalled, and not a zombie waiting to be reaped. */
export function alive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false
  try {
    process.kill(pid, 0)
    const state = execFileSync('/bin/ps', ['-p', String(pid), '-o', 'stat='], {
      encoding: 'utf8',
    }).trim()
    return state.length > 0 && !state.startsWith('Z')
  } catch {
    return false
  }
}

/** Polls `check` until it answers something other than nothing or false, or `timeoutMs` pass. */
export async function until(label, check, timeoutMs = TIMEOUT_MS) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const result = check()
    if (result !== null && result !== undefined && result !== false) return result
    if (Date.now() >= deadline) throw new Error(`${label} did not happen within ${timeoutMs} ms`)
    await new Promise((wake) => setTimeout(wake, 100))
  }
}
