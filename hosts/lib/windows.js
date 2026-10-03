/**
 * How each harness's own window opens on a conversation ConsensFlow starts or
 * resumes: the command, its arguments, the environment it needs and the keys
 * it must not inherit. The daemon's adapters build every pane from here.
 */

/** Control variables of an outer ConsensFlow process that a window must never inherit. */
const STRIPPED_CONTROL_ENV = new Set([
  'CONSENSFLOW_APP',
  'CONSENSFLOW_APP_TOKEN',
  'CONSENSFLOW_TAB',
  'CONSENSFLOW_PANE_ID',
  'CONSENSFLOW_LEAD_ID',
  'CONSENSFLOW_LAUNCH',
])

/** The billing guard a window carries: a key whose presence would silently switch billing. */
function interactiveGuards(kind) {
  if (kind === 'claude-code') return ['ANTHROPIC_API_KEY']
  if (kind === 'codex') return ['OPENAI_API_KEY']
  return []
}

/**
 * Every window, fresh or resumed, for every role, opens in full-permission
 * ("yolo") mode (the owner's decision, 2026-09-19). Each flag is the harness's
 * own documented one; Claude's settings file adds the mode's companions.
 */
const YOLO = {
  claude: ['--permission-mode', 'bypassPermissions'],
  codex: ['--dangerously-bypass-approvals-and-sandbox'],
  opencode: ['--auto'],
  pi: ['--approve'],
  devin: ['--permission-mode', 'dangerous', '--respect-workspace-trust', 'false'],
}

/** The environment a window runs with: the base plus what it declares, minus what it must not see. */
export function childEnv(base, { env: envOverrides, dropEnv } = {}) {
  const env = { ...base, ...(envOverrides ?? {}) }
  for (const key of dropEnv ?? []) delete env[key]
  for (const key of Object.keys(env)) {
    if (
      key.startsWith('CMUX_SOCKET') ||
      key === 'CMUX_CLAUDE_HOOK_CMUX_BIN' ||
      STRIPPED_CONTROL_ENV.has(key)
    ) {
      delete env[key]
    }
  }
  return env
}

/**
 * The harness's OWN interactive window on a conversation: one we start, or
 * one it already keeps (the session a run leaves behind is the one each TUI
 * can open, whole history included: `codex resume`, not `codex exec
 * resume`). Both open on the agent's model and effort with the same flags: a
 * resumed window without them ran on the harness's own default (Codex on
 * whatever ~/.codex/config.toml names). `seed` is the first message where
 * the TUI takes one; OpenCode's go through its native API after the window
 * starts, because its `--session` launch ignores `--prompt`. Null when the
 * harness needs an id and has none, or for a kind with no window of its own.
 */
function interactiveWindow(agent, sessionId, seed, resume) {
  const seeded = (args) => (seed ? [...args, seed] : args)
  switch (agent.kind) {
    case 'claude-code':
      if (!sessionId) return null
      return {
        command: 'claude',
        args: seeded([
          resume ? '--resume' : '--session-id',
          sessionId,
          ...modelAndEffort(agent),
          ...YOLO.claude,
        ]),
        env: {},
        dropEnv: interactiveGuards('claude-code'),
      }
    case 'pi':
      // `--session-id` creates the session the first time and resumes it after.
      if (!sessionId) return null
      return {
        command: 'pi',
        args: seeded(['--session-id', sessionId, ...modelAndEffort(agent), ...YOLO.pi]),
        env: {},
        dropEnv: [],
      }
    case 'codex':
      // `codex [PROMPT]` opens the real window seeded with that prompt; it
      // announces no id, so the broker names the thread once Codex starts it.
      return {
        command: 'codex',
        args: seeded([
          ...(resume ? ['resume', sessionId] : []),
          ...modelAndEffort(agent),
          ...YOLO.codex,
        ]),
        env: {},
        dropEnv: interactiveGuards('codex'),
      }
    case 'devin':
      return {
        command: 'devin',
        args: [...(resume ? ['--resume', sessionId] : []), ...modelAndEffort(agent), ...YOLO.devin],
        ...(seed ? { prompt: seed } : {}),
        env: {},
        dropEnv: [],
      }
    case 'opencode':
      // OpenCode keeps the model and its variant with the session, and they
      // are read back for a resumed one: only a new session is given them.
      return {
        command: 'opencode',
        args: [
          ...(sessionId ? ['--session', sessionId] : []),
          ...(resume ? [] : modelAndEffort(agent)),
          ...YOLO.opencode,
        ],
        env: {},
        dropEnv: [],
      }
    default:
      return null
  }
}

/** The flags that put a window on its agent's model and effort. */
function modelAndEffort({ kind, model, effort, thinking }) {
  switch (kind) {
    case 'claude-code':
      return [...(model ? ['--model', model] : []), ...(effort ? ['--effort', effort] : [])]
    case 'codex':
      return [
        ...(model ? ['--model', model] : []),
        ...(effort ? ['-c', `model_reasoning_effort="${effort}"`] : []),
      ]
    case 'pi':
      return [...(model ? ['--model', model] : []), ...(thinking ? ['--thinking', thinking] : [])]
    case 'devin':
      // Devin writes the level into the id (claude-opus-5-5-max): an agent
      // names the family and its effort, joined here. No model is Devin's own
      // setting, which a chief from before every chief had an agent still runs on.
      return model ? ['--model', effort ? `${model}-${effort}` : model] : []
    case 'opencode':
      return model ? ['--model', model] : []
    default:
      return []
  }
}

/** A window on a conversation that does not exist yet (Claude and Pi take the id from us). */
export const interactiveStart = (agent, sessionId, seed) =>
  interactiveWindow(agent, sessionId, seed, false)

/** A window on a conversation the harness already keeps, by the id it recorded. */
export const interactiveResume = (agent, sessionId, seed) =>
  sessionId ? interactiveWindow(agent, sessionId, seed, true) : null
