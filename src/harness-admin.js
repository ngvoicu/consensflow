import { execFile } from 'node:child_process'
import { readFileSync, realpathSync } from 'node:fs'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { harnessPath, knownHarnesses } from './harnesses.js'
import { preparePiExtension } from './pi-install.js'

const execute = promisify(execFile)
const SOURCES = {
  claude: [
    'https://registry.npmjs.org/@anthropic-ai/claude-code/latest',
    'https://code.claude.com/docs/en/setup',
  ],
  codex: ['https://registry.npmjs.org/@openai/codex/latest', 'https://github.com/openai/codex'],
  opencode: ['https://registry.npmjs.org/opencode-ai/latest', 'https://opencode.ai/docs/cli/'],
  pi: ['https://registry.npmjs.org/@earendil-works/pi-coding-agent/latest', 'https://pi.dev/'],
  kimi: ['https://pypi.org/pypi/kimi-cli/json', 'https://github.com/MoonshotAI/kimi-cli'],
}

export function releaseSource(id, executable, env) {
  let path = executable
  try {
    path = realpathSync(executable)
  } catch {}
  const brew = path?.match(/\/(Caskroom|Cellar)\/([^/]+)\//)
  const expected = id === 'claude' ? ['claude-code', 'claude-code@latest'] : [id]
  if (brew && expected.includes(brew[2])) {
    const type = brew[1] === 'Caskroom' ? 'cask' : 'formula'
    return {
      url: `https://formulae.brew.sh/api/${type}/${brew[2]}.json`,
      format: type,
      distribution: `Homebrew ${brew[2]}`,
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
      distribution: `Claude native ${channel}`,
    }
  }
  return {
    url: SOURCES[id][0],
    format: id === 'kimi' ? 'pypi' : 'npm',
    distribution: path?.includes('/node_modules/')
      ? 'npm'
      : id === 'kimi'
        ? 'PyPI'
        : 'publisher release',
  }
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

export function integrationEvidence(id, tabs, deliveries) {
  const kind = id === 'claude' ? 'claude-code' : id
  for (const tab of tabs) {
    if (tab.closed || tab.role === 'pm' || tab.lead?.harness !== kind) continue
    const pane = tab.panes.find((pane) => pane.kind === 'lead')
    const receipt = deliveries.find(
      (record) =>
        record.state === 'accepted' &&
        Array.isArray(record.evidenceIds) &&
        record.evidenceIds.length > 0 &&
        record.target?.tab === tab.id &&
        record.target.pane === pane?.id &&
        record.target.generation === tab.lead.generation &&
        record.target.session === tab.lead.nativeSession,
    )
    if (receipt)
      return {
        state: 'ok',
        reason: `A complete worker result was observed in the current lead (${tab.roleName ?? tab.id})`,
        checkedAt: receipt.acceptedAt,
      }
  }
  return {
    state: 'unverified',
    reason: 'No complete result receipt has been verified for a currently running lead',
  }
}

/** Diagnostics never participate in launch, binding, reading or delivery decisions. */
export class HarnessAdmin {
  #env
  #latest
  #integration
  #cache = new Map()
  #pending = new Map()
  constructor(env, { latest = latestRelease, integration = null } = {}) {
    this.#env = env
    this.#latest = latest
    this.#integration = integration
  }

  async check(id = null, { refresh = false } = {}) {
    const ids = knownHarnesses(this.#env).map((row) => row.id)
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
      instructions: SOURCES[id][1],
      version: { state: 'not-installed' },
      update: { state: 'not-checked' },
      integration: { state: 'unverified', reason: 'No live integration evidence has been checked' },
    }
    if (!path) return row
    try {
      const { stdout } = await execute(path, ['--version'], {
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
    const source = releaseSource(id, path, this.#env)
    row.distribution = source.distribution
    try {
      const value = await this.#latest(id, source)
      const comparison = newer(row.version.value, value)
      row.update = {
        state: comparison === null ? 'unknown' : comparison ? 'available' : 'current',
        value,
        source: source.url,
        note:
          source.distribution === 'publisher release'
            ? 'Publisher release; installation channel could not be identified'
            : `Release from ${source.distribution}`,
      }
    } catch (error) {
      row.update = { state: 'error', reason: error.message }
    }
    if (this.#integration) row.integration = await this.#integration(id)
    if (id === 'pi') {
      row.extension = preparePiExtension(this.#env)
      if (row.extension.state === 'error')
        row.integration = { state: 'error', reason: row.extension.reason }
    }
    return row
  }
}
