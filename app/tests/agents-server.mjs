import { spawn } from 'node:child_process'
import { mkdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { assertBuilt, NATIVE_CF } from '../../tests/choice.mjs'
import { fakeExecutable, windowsEnv } from '../../tests/helpers.mjs'

const INSTALLED = ['claude', 'codex', 'pi', 'devin']

/**
 * The daemon's screen the way it serves it: Harnesses, behind the API, opened with the UI
 * token (the saved agents' screen is no more). The daemon is
 * the native `cf ui` of this checkout, on the home `env` gives. A harness named
 * in `installed` is a stand-in on its PATH that says it is 1.2.3, and
 * `runs(harness)` counts how often it was run. The feeds that say what the
 * latest release of a harness is are the real ones: what the screens say of
 * them is for no test to depend on.
 */
export async function agentsServer(env, { installed = INSTALLED } = {}) {
  assertBuilt()
  mkdirSync(env.CONSENSFLOW_HOME, { recursive: true })
  // A harness's version is asked in the human's home.
  mkdirSync(env.HOME, { recursive: true })
  // The harnesses installed here: stand-ins on PATH.
  mkdirSync(env.PATH, { recursive: true })
  const logs = join(dirname(env.PATH), 'runs')
  mkdirSync(logs, { recursive: true })
  for (const command of installed) {
    fakeExecutable(join(env.PATH, command), { output: '1.2.3', log: join(logs, command) })
  }
  const child = spawn(NATIVE_CF, ['ui', '--json', '--no-open'], {
    env: { ...windowsEnv(), ...env },
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const exited = new Promise((resolve) => child.once('exit', resolve))
  let errors = ''
  child.stderr.on('data', (chunk) => {
    errors += chunk
  })
  // Its handle is the first line it says: where it listens, and the app's token.
  const handle = await new Promise((resolve, reject) => {
    let said = ''
    const onData = (chunk) => {
      said += chunk
      const end = said.indexOf('\n')
      if (end === -1) return
      child.stdout.off('data', onData)
      resolve(JSON.parse(said.slice(0, end)))
    }
    child.stdout.on('data', onData)
    exited.then(() => reject(new Error(`the daemon ended before it said it was ready: ${errors}`)))
  })
  // What it says after the handle is the bridge's, with nobody on the other end.
  child.stdout.resume()
  return {
    // The address without its closing slash: the pages are named after it.
    url: handle.url.replace(/\/$/, ''),
    token: handle.token,
    /** How many times the stand-in for `harness` was run. */
    runs(harness) {
      try {
        return readFileSync(join(logs, harness), 'utf8').split('\n').filter(Boolean).length
      } catch {
        return 0
      }
    },
    async close() {
      child.stdin.end()
      const stuck = setTimeout(() => child.kill('SIGKILL'), 5_000)
      await exited
      clearTimeout(stuck)
    },
  }
}
