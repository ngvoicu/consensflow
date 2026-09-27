import { statSync } from 'node:fs'

/**
 * The pure part of an eval run: which agents make up the staff for a set of
 * harnesses, what environment gives a chief its model where the harness
 * takes one from the environment, and how the scripted human answers a
 * question. Everything here is tested without spending a token.
 */

/**
 * One cheap model per harness (brain: operations/test-models.md), for the
 * staff and, on OpenCode, for the chief. OpenCode's free Muse Spark died
 * mid-run twice out of two (a dead turn), so OpenCode runs a paid Go model.
 */
export const HARNESSES = {
  claude: { kind: 'claude-code', model: 'claude-haiku-4-5-20251001' },
  codex: { kind: 'codex', model: 'gpt-5.6-luna' },
  pi: { kind: 'pi', model: 'opencode-go/muse-spark-1.3-contributor' },
  opencode: { kind: 'opencode', model: 'opencode-go/deepseek-v4-flash' },
  devin: { kind: 'devin', model: 'swe-1-6-slow' },
}

/** The roles a staff harness fills; two workers, so parallel work has somewhere to run. */
const ROLES = [
  ['worker', 'worker'],
  ['worker-2', 'worker'],
  ['advisor', 'advisor'],
  ['reviewer', 'reviewer'],
]

/**
 * The roster rows and the project staff for these harnesses: for each, two
 * workers, an advisor and a reviewer on its cheap model (`models` overrides
 * a harness's model). Every member is standard tier, so the daemon picks
 * among them by its own rule and any harness may get any task. `effort` is
 * each member's reasoning level (Pi calls it thinking); Devin has none.
 */
export function staffFor(harnesses, models = {}, effort = undefined) {
  const agents = []
  const staff = []
  for (const name of harnesses) {
    const harness = HARNESSES[name]
    if (harness === undefined) throw new Error(`no such eval harness: ${name}`)
    for (const [suffix, role] of ROLES) {
      const id = `eval-${name}-${suffix}`
      agents.push({
        id,
        kind: harness.kind,
        model: models[name] ?? harness.model,
        workTier: 'standard',
        ...(effort === undefined || name === 'devin'
          ? {}
          : { [name === 'pi' ? 'thinking' : 'effort']: effort }),
      })
      staff.push({ agent: id, roles: [role] })
    }
  }
  return { agents, staff }
}

/**
 * The chief has no model of its own in the roster: it runs its harness's
 * default. Claude Code and OpenCode take one from the environment (Opus, and
 * OpenCode's cheap model, unless `model` says otherwise), Codex from the
 * eval's Codex wrapper (its cheap model unless `model` says otherwise); Pi
 * and Devin run the model their own configuration names, so `model` is
 * ignored there and the report says so.
 */
export function chiefEnvironment(chief, model = undefined) {
  if (chief === 'claude') {
    const chosen = model ?? 'claude-opus-5'
    return { env: { ANTHROPIC_MODEL: chosen }, model: chosen }
  }
  if (chief === 'opencode') {
    const chosen = model ?? HARNESSES.opencode.model
    return { env: { OPENCODE_CONFIG_CONTENT: JSON.stringify({ model: chosen }) }, model: chosen }
  }
  // Codex takes it through the eval's Codex wrapper (`-c model=…`, see run.mjs).
  if (chief === 'codex') return { env: {}, model: model ?? HARNESSES.codex.model }
  if (!(chief in HARNESSES)) throw new Error(`no such eval harness: ${chief}`)
  return { env: {}, model: `${chief}'s default` }
}

/**
 * Where Claude Code keeps what it saves about a folder (`~/.claude/projects/
 * <key>`): the folder's path with every slash and dot made a dash. Its
 * `memory/` there is read by every later session in that folder, so a run's
 * chief would learn from the last run's; the runner clears it first.
 */
export function claudeProjectKey(directory) {
  return directory.replace(/[/.]/g, '-')
}

/**
 * The last `count` non-empty lines a window printed, control sequences already
 * stripped: near enough its screen, for a report to show why a chief said
 * nothing (a login page, a quota wall, a model that does not exist).
 */
export function lastLines(text, count = 40) {
  return text
    .split(/\r\n|\r|\n/)
    .map((line) => line.trimEnd())
    .filter((line) => line.trim() !== '')
    .slice(-count)
}

/**
 * The scripted owner's answer, shaped as the board sends it. A question with
 * options gets `choices`, one pick per sub-question as the board's form
 * does: the first of the scenario's `answers` whose pattern matches that
 * sub-question (free text, like the form's "Something else"), else its first
 * option, else the fallback. A plain question gets a `body`: the first
 * pattern that matches its first line (its subject), then the first that
 * matches anywhere in it, then the fallback.
 */
export function answerFor(scenario, question) {
  const answers = scenario.answers ?? []
  const matching = (text) => answers.find(({ match }) => match.test(text))?.text
  if (question.questions !== null && question.questions.length > 0) {
    return {
      choices: question.questions.map((q) => [
        matching(`${q.header ?? ''} ${q.question ?? ''}`) ??
          q.options?.[0]?.label ??
          scenario.fallback,
      ]),
    }
  }
  return {
    body: matching(question.body.split('\n')[0]) ?? matching(question.body) ?? scenario.fallback,
  }
}

/**
 * The real `name` (claude, codex) on this PATH, skipping the eval's own
 * wrapper directory, so a wrapper can exec it by absolute path.
 */
export function realOnPath(name, pathVariable, exists = defaultExists) {
  for (const dir of pathVariable.split(':')) {
    if (dir === '' || dir.endsWith('/evals/bin')) continue
    const candidate = `${dir.replace(/\/$/, '')}/${name}`
    if (exists(candidate)) return candidate
  }
  throw new Error(`${name} is not on PATH`)
}

/**
 * The `-c` overrides that switch off every MCP server Codex would start
 * (`codex mcp list --json`): each gets a harmless, disabled definition,
 * which also covers servers a plugin or the ChatGPT app adds outside
 * config.toml (a bare `enabled=false` is refused for those).
 */
export function codexIsolation(servers) {
  return servers.flatMap(({ name }) => {
    // Codex's -c takes the key's segments literally: a quoted name would
    // define a new server and leave the real one on. Refuse, never half-isolate.
    if (!/^[A-Za-z0-9_-]+$/.test(name)) {
      throw new Error(`cannot switch off the Codex MCP server ${JSON.stringify(name)}`)
    }
    return [
      '-c',
      `mcp_servers.${name}.command="/usr/bin/true"`,
      '-c',
      `mcp_servers.${name}.enabled=false`,
    ]
  })
}

function defaultExists(file) {
  try {
    return statSync(file).isFile()
  } catch {
    return false
  }
}
