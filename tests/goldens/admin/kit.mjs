/**
 * What the admin's scenarios are made of: where a harness's CLI is found on
 * this system, and the answers the world gives.
 */
import { delimiter } from 'node:path'

export const WINDOWS = process.platform === 'win32'

/**
 * The file a CLI is found at here: Windows finds `codex.exe`, never the bare
 * name. Scripted programs are never run, so what the file holds does not
 * matter, and an `.exe` runs as it is where a `.cmd` would go through cmd.exe.
 */
export const cli = (path) => (WINDOWS ? `${path}.exe` : path)

/** A CLI the system may run, at `path`. */
export const exe = (path, extra = {}) => ({ path: cli(path), executable: true, ...extra })

/** A PATH of these folders. */
export const onPath = (...folders) => folders.join(delimiter)

/** What a program that ended with 0 wrote. */
export const said = (stdout, stderr = '') => ({ stdout, stderr })

/** A program that did not answer, as `execFile` rejects it. */
export const failed = (message, extra = {}) => ({
  error: { message, killed: false, stdout: '', stderr: '', ...extra },
})

/** A program that was ended at its timeout, as `execFile` rejects it. */
export const timedOut = (stdout = '', stderr = '') => ({
  error: { message: 'Command failed: x\n', killed: true, stdout, stderr },
})

/** The release a feed says. */
export const release = (value) => ({ value })

/** The feed's own address of each harness, as `SOURCES` has them. */
export const FEEDS = {
  claude: 'https://registry.npmjs.org/@anthropic-ai/claude-code/latest',
  codex: 'https://registry.npmjs.org/@openai/codex/latest',
  opencode: 'https://registry.npmjs.org/opencode-ai/latest',
  pi: 'https://registry.npmjs.org/@earendil-works/pi-coding-agent/latest',
  devin: 'https://static.devin.ai/cli/current/manifest.json',
}
