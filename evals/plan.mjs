/**
 * The pure part of an eval run: which agents make up the staff for a set of
 * harnesses, what environment gives a chief its model where the harness
 * takes one from the environment, and how the scripted human answers a
 * question. Everything here is tested without spending a token.
 */

/** One cheap model per harness (brain: operations/test-models.md), for the staff. */
export const HARNESSES = {
  claude: { kind: 'claude-code', model: 'claude-haiku-4-5-20251001' },
  codex: { kind: 'codex', model: 'gpt-5.6-luna' },
  pi: { kind: 'pi', model: 'opencode-go/muse-spark-1.3-contributor' },
  opencode: { kind: 'opencode', model: 'opencode/muse-spark-1.3-contributor-free' },
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
 * among them by its own rule and any harness may get any task.
 */
export function staffFor(harnesses, models = {}) {
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
      })
      staff.push({ agent: id, roles: [role] })
    }
  }
  return { agents, staff }
}

/**
 * The chief has no model of its own in the roster: it runs its harness's
 * default. Claude Code and OpenCode take one from the environment (Opus, and
 * OpenCode's cheap model, unless `model` says otherwise); Codex, Pi and Devin
 * run the model their own configuration names, so `model` is ignored there
 * and the report says so.
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
 * The scripted human's answer: the first of the scenario's `answers` whose
 * pattern matches the question wins, free text or not; then a question with
 * options takes each one's first option; then the scenario's `fallback`.
 */
export function answerFor(scenario, question) {
  for (const { match, text } of scenario.answers ?? []) {
    if (match.test(question.body)) return text
  }
  if (question.questions !== null && question.questions.length > 0) {
    return question.questions.map((q) => q.options?.[0]?.label ?? scenario.fallback).join('\n')
  }
  return scenario.fallback
}
