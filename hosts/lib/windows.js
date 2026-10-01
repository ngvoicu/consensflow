import { validateKimiEffort } from './presets.js'

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
  'CF_RESULT_RECEIVER',
  'CF_RESULT_SIGNAL',
])

// Every window carries this marker so ConsensFlow tooling running inside it
// (the Claude Code hooks, the Pi extension, cf itself) can tell it is nested.
const CHILD_ENV = { CONSENSFLOW_CHILD: '1' }

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
  kimi: ['--auto'],
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
 * The harness's OWN interactive command for a conversation we started: the
 * session a run leaves behind is the one each TUI can open, whole history
 * included. That is `codex resume`, not `codex exec resume`. `seed` is an
 * optional first message where the TUI accepts one; OpenCode tasks go through
 * its native API after the window starts, because its `--session` launch
 * ignores `--prompt`. Null when the harness cannot resume, or there is no id.
 */
export function interactiveResume(agent, sessionId, seed) {
  if (!sessionId) return null
  const withSeed = (args) => (seed ? [...args, seed] : args)
  switch (agent.kind) {
    case 'devin':
      return {
        command: 'devin',
        args: ['--resume', sessionId, ...YOLO.devin],
        ...(seed ? { prompt: seed } : {}),
        env: { ...CHILD_ENV },
        dropEnv: [],
      }
    case 'codex':
    case 'image':
      return {
        command: 'codex',
        args: withSeed(['resume', sessionId, ...YOLO.codex]),
        env: { ...CHILD_ENV },
        dropEnv: interactiveGuards('codex'),
      }
    case 'claude-code': {
      const args = ['--resume', sessionId]
      if (agent.model) args.push('--model', agent.model)
      args.push(...YOLO.claude)
      return {
        command: 'claude',
        args: withSeed(args),
        env: { ...CHILD_ENV },
        dropEnv: interactiveGuards('claude-code'),
      }
    }
    case 'pi': {
      const args = ['--session-id', sessionId]
      if (agent.model) args.push('--model', agent.model)
      args.push(...YOLO.pi)
      return { command: 'pi', args: withSeed(args), env: { ...CHILD_ENV }, dropEnv: [] }
    }
    case 'opencode': {
      const args = ['--session', sessionId, ...YOLO.opencode]
      return { command: 'opencode', args, env: { ...CHILD_ENV }, dropEnv: [] }
    }
    case 'kimi': {
      validateKimiEffort(agent)
      // `-S <id>` without `-p` IS the interactive window on that session. No
      // flag seeds its first message, so a follow-up sent this way arrives
      // as a pane the user types into.
      return {
        command: 'kimi',
        args: ['-S', sessionId, ...YOLO.kimi],
        env: {
          ...CHILD_ENV,
          ...(agent.effort ? { KIMI_MODEL_THINKING_EFFORT: agent.effort } : {}),
        },
        dropEnv: [],
      }
    }
    default:
      return null
  }
}

/**
 * The harness's OWN window on a conversation that does not exist yet. Claude
 * and Pi take the id from us (`--session-id`, minted by the caller); OpenCode
 * opens the empty session its native API created; Codex opens on a positional
 * seed and its own metadata identifies the thread afterwards. Kimi cannot
 * seed a window and an image agent has none: null.
 */
export function interactiveStart(agent, sessionId, seed) {
  switch (agent.kind) {
    case 'devin':
      return {
        command: 'devin',
        args: [
          // Devin writes the level into the id (claude-opus-5-5-max): an agent
          // names the family and its effort, joined here.
          ...(agent.model && agent.model !== 'default'
            ? ['--model', agent.effort ? `${agent.model}-${agent.effort}` : agent.model]
            : []),
          ...YOLO.devin,
        ],
        ...(seed ? { prompt: seed } : {}),
        env: { ...CHILD_ENV },
        dropEnv: [],
      }
    case 'claude-code': {
      if (!sessionId) return null
      const args = ['--session-id', sessionId]
      if (agent.model) args.push('--model', agent.model)
      if (agent.effort) args.push('--effort', agent.effort)
      args.push(...YOLO.claude)
      if (seed) args.push(seed)
      return {
        command: 'claude',
        args,
        env: { ...CHILD_ENV },
        dropEnv: interactiveGuards('claude-code'),
      }
    }
    case 'pi': {
      if (!sessionId) return null
      const args = ['--session-id', sessionId]
      if (agent.model) args.push('--model', agent.model)
      if (agent.thinking) args.push('--thinking', agent.thinking)
      args.push(...YOLO.pi)
      if (seed) args.push(seed)
      return { command: 'pi', args, env: { ...CHILD_ENV }, dropEnv: [] }
    }
    case 'opencode': {
      const args = []
      if (sessionId) args.push('--session', sessionId)
      if (agent.model) args.push('--model', agent.model)
      args.push(...YOLO.opencode)
      if (seed && !sessionId) args.push('--prompt', seed)
      return { command: 'opencode', args, env: { ...CHILD_ENV }, dropEnv: [] }
    }
    case 'codex':
    case 'image': {
      // `codex [PROMPT]` opens the real window seeded with that prompt; it
      // announces no id, so the caller finds the thread in Codex's own store.
      // An image agent is Codex too, on its own default model: its image
      // tool draws, whatever reasoning model answers.
      const args = []
      if (agent.kind === 'codex' && agent.model) args.push('--model', agent.model)
      if (agent.kind === 'codex' && agent.effort) {
        args.push('-c', `model_reasoning_effort="${agent.effort}"`)
      }
      args.push(...YOLO.codex)
      if (seed) args.push(seed)
      return { command: 'codex', args, env: { ...CHILD_ENV }, dropEnv: interactiveGuards('codex') }
    }
    default:
      return null
  }
}
