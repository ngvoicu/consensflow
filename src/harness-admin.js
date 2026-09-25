import { execFile } from 'node:child_process'
import { readFileSync, realpathSync } from 'node:fs'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { DEVIN_MINIMUM_VERSION, supportedDevinVersion } from './devin-install.js'
import { harnessPath, knownHarnesses, runnable } from './harnesses.js'
import { prepareOpenCodeExtension } from './opencode-install.js'
import { preparePiExtension } from './pi-install.js'

const execute = promisify(execFile)
const SOURCES = {
  claude: 'https://registry.npmjs.org/@anthropic-ai/claude-code/latest',
  codex: 'https://registry.npmjs.org/@openai/codex/latest',
  opencode: 'https://registry.npmjs.org/opencode-ai/latest',
  pi: 'https://registry.npmjs.org/@earendil-works/pi-coding-agent/latest',
  kimi: 'https://pypi.org/pypi/kimi-cli/json',
  devin: 'https://static.devin.ai/cli/current/manifest.json',
}

const NAME = {
  claude: 'Claude',
  codex: 'Codex',
  opencode: 'OpenCode',
  pi: 'Pi',
  kimi: 'Kimi',
  devin: 'Devin',
}
const NPM_PACKAGES = {
  claude: '@anthropic-ai/claude-code',
  codex: '@openai/codex',
  opencode: 'opencode-ai',
  pi: '@earendil-works/pi-coding-agent',
}
/** Each harness's own installer: where it puts the CLI, and its command that updates it. */
const OWN_INSTALLER = {
  codex: { marker: '/.codex/bin/', update: ['update'] },
  opencode: { marker: '/.opencode/bin/', update: ['upgrade'] },
  pi: { marker: '/.pi/bin/', update: ['update', '--self'] },
  devin: { marker: '/devin/cli/', update: ['update'] },
}

/**
 * How a harness got onto this machine, read from where its executable really
 * lives: the release feed to compare against, the words for the page, and the
 * command that brings it to the latest release the same way (null when the
 * method is not recognized, so the human updates it as they installed it).
 */
export function releaseSource(id, executable, env) {
  let path = executable
  try {
    path = realpathSync(executable)
  } catch {}
  // The layouts below are spelled with `/`; Windows answers with `\`.
  path = path?.replaceAll('\\', '/')
  const brew = path?.match(/^(.*)\/(Caskroom|Cellar)\/([^/]+)\//)
  const expected = id === 'claude' ? ['claude-code', 'claude-code@latest'] : [id]
  if (brew && expected.includes(brew[3])) {
    const cask = brew[2] === 'Caskroom'
    return {
      url: `https://formulae.brew.sh/api/${cask ? 'cask' : 'formula'}/${brew[3]}.json`,
      format: cask ? 'cask' : 'formula',
      distribution: 'Homebrew',
      update: [join(brew[1], 'bin', 'brew'), 'upgrade', ...(cask ? ['--cask'] : []), brew[3]],
    }
  }
  if (id === 'claude' && path?.includes('/claude/versions/')) {
    let channel = 'latest'
    try {
      const settings = JSON.parse(
        readFileSync(
          join(env.CLAUDE_CONFIG_DIR ?? join(env.HOME, '.claude'), 'settings.json'),
          'utf8',
        ),
      )
      if (settings.autoUpdatesChannel === 'stable') channel = 'stable'
    } catch {}
    return {
      url: `https://downloads.claude.ai/claude-code-releases/${channel}`,
      format: 'text',
      distribution: `Claude's installer, ${channel} channel`,
      update: [executable, 'update'],
    }
  }
  const npm = path?.match(/^(.*)\/lib\/node_modules\//)
  const source = { url: SOURCES[id], format: id === 'kimi' ? 'pypi' : 'npm' }
  if (npm && NPM_PACKAGES[id]) {
    return {
      ...source,
      distribution: 'npm',
      update: [join(npm[1], 'bin', 'npm'), 'install', '-g', `${NPM_PACKAGES[id]}@latest`],
    }
  }
  const own = OWN_INSTALLER[id]
  if (own && path?.includes(own.marker)) {
    return {
      ...source,
      distribution: `${NAME[id]}'s installer`,
      update: [executable, ...own.update],
    }
  }
  return { ...source, distribution: null, update: null }
}

function versionOf(text) {
  return String(text).match(/(?:^|\s|v)(\d+\.\d+\.\d+(?:-[\w.-]+)?)(?=\s|$|\))/)?.[1] ?? null
}

function newer(local, remote) {
  if (!/^\d+\.\d+\.\d+$/.test(local ?? '') || !/^\d+\.\d+\.\d+$/.test(remote ?? '')) return null
  const a = local.split('.').map(Number),
    b = remote.split('.').map(Number)
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return b[i] > a[i]
  return false
}

async function latestRelease(_id, source) {
  const response = await fetch(source.url, {
    signal: AbortSignal.timeout(5000),
    redirect: 'error',
  })
  if (!response.ok) throw new Error(`Release service returned HTTP ${response.status}`)
  let text = ''
  for await (const bytes of response.body) {
    text += Buffer.from(bytes).toString('utf8')
    if (text.length > 2_000_000) throw new Error('Release metadata exceeds size limit')
  }
  const data = source.format === 'text' ? null : JSON.parse(text)
  const value =
    source.format === 'text'
      ? text.trim()
      : source.format === 'formula'
        ? data.versions?.stable
        : source.format === 'pypi'
          ? data.info?.version
          : data.version
  if (typeof value !== 'string' || !versionOf(value))
    throw new Error('Release version is unavailable')
  return value
}

/** Diagnostics never participate in launch, binding, reading or delivery decisions. */
export class HarnessAdmin {
  #env
  #latest
  #run
  #cache = new Map()
  #pending = new Map()
  constructor(env, { latest = latestRelease, run = execute } = {}) {
    this.#env = env
    this.#latest = latest
    this.#run = run
  }

  /**
   * Brings a harness to its latest release the way it was installed (its own
   * updater, Homebrew or npm), then checks it again and says what happened:
   * updated, unchanged, failed (with the tool's last lines), or unsupported
   * when the install method is not recognized.
   */
  async update(id) {
    const [row] = await this.check(id)
    if (!row.installed) throw new Error(`${NAME[id]} is not installed`)
    const source = releaseSource(id, row.path, this.#env)
    if (source.update === null) {
      return {
        id,
        state: 'unsupported',
        reason: `ConsensFlow does not recognize how ${NAME[id]} was installed here: update it the way you installed it.`,
        harness: row,
      }
    }
    const before = row.version.value ?? null
    const command = source.update.join(' ')
    let output = ''
    let failure = null
    try {
      const run = runnable(source.update[0], source.update.slice(1), this.#env)
      const result = await this.#run(run.file, run.args, {
        ...run.options,
        env: this.#env,
        cwd: this.#env.HOME,
        timeout: 600_000,
        maxBuffer: 1_000_000,
        windowsHide: true,
      })
      output = `${result.stdout ?? ''}${result.stderr ?? ''}`
    } catch (error) {
      output = `${error.stdout ?? ''}${error.stderr ?? ''}`
      failure = error.killed ? 'the update ran for ten minutes and was stopped' : error.message
    }
    const [after] = await this.check(id, { refresh: true })
    return {
      id,
      state: failure !== null ? 'failed' : after.version.value !== before ? 'updated' : 'unchanged',
      before,
      after: after.version.value ?? null,
      command,
      output: output.trim().split('\n').slice(-20).join('\n').slice(-2000),
      ...(failure === null ? {} : { reason: failure }),
      harness: after,
    }
  }

  async check(id = null, { refresh = false } = {}) {
    const ids = knownHarnesses().map((row) => row.id)
    if (id !== null && !ids.includes(id)) throw new Error('Unknown harness')
    return Promise.all(
      (id ? [id] : ids).map(async (name) => {
        const path = harnessPath(name, this.#env)
        const cached = this.#cache.get(name)
        if (!refresh && cached?.path === path && Date.now() - cached.checkedAt < 300_000)
          return cached
        if (this.#pending.has(name)) return this.#pending.get(name)
        const pending = this.#inspect(name, path).then((row) => {
          this.#cache.set(name, row)
          return row
        })
        this.#pending.set(name, pending)
        try {
          return await pending
        } finally {
          this.#pending.delete(name)
        }
      }),
    )
  }

  async #inspect(id, path) {
    const row = {
      id,
      path,
      installed: Boolean(path),
      lead: id !== 'kimi',
      checkedAt: Date.now(),
      version: { state: 'not-installed' },
      update: { state: 'not-checked' },
    }
    if (!path) return row
    try {
      const run = runnable(path, ['--version'], this.#env)
      const { stdout } = await execute(run.file, run.args, {
        ...run.options,
        env: this.#env,
        cwd: this.#env.HOME,
        timeout: 3000,
        maxBuffer: 8192,
        windowsHide: true,
      })
      const value = versionOf(stdout)
      row.version = value
        ? { state: 'checked', value }
        : { state: 'unknown', reason: 'Version output was not recognized' }
    } catch (error) {
      row.version = {
        state: 'error',
        reason: error.killed ? 'Version check timed out' : 'Version command failed',
      }
    }
    if (id === 'devin')
      row.setup = supportedDevinVersion(row.version.value)
        ? { state: 'ready' }
        : {
            state: 'update-required',
            reason: `Devin ${DEVIN_MINIMUM_VERSION} or newer is required. Update Devin before opening a pane.`,
          }
    const source = releaseSource(id, path, this.#env)
    row.distribution = source.distribution
    const command = source.update === null ? null : source.update.join(' ')
    try {
      const value = await this.#latest(id, source)
      const comparison = newer(row.version.value, value)
      row.update = {
        state: comparison === null ? 'unknown' : comparison ? 'available' : 'current',
        value,
        source: source.url,
        command,
      }
    } catch (error) {
      row.update = { state: 'error', reason: error.message, command }
    }
    if (id === 'pi') row.extension = preparePiExtension(this.#env)
    if (id === 'opencode') row.extension = prepareOpenCodeExtension(this.#env)
    return row
  }
}
