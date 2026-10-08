import { ask, build } from './rust-channels.mjs'

/**
 * The harness code of crates/cf-harness in Rust, asked what the evals and the
 * live tools need of it: they start the real harnesses themselves, in windows
 * of the pane host, and need what the daemon needs to open one the way the app
 * does and to read what comes of it. Each function here is one question to the
 * test binary `harness-ask` (`cf_harness::tooling` says what it asks and
 * answers), built when the first is asked.
 */

let executable = null

/** One question to the test binary, and its answer. */
function question(body) {
  executable ??= build('harness-ask')
  return ask(executable, body)
}

/**
 * What the harness's own record of the conversation `session` says, read
 * whole in the places `env` names (a variable given null is one the caller
 * removed, and the binary leaves it out): its items, whether the window's turn
 * is over as the dispatcher reads it, and the record's word on the turn
 * (`settlement`: unknown, in-flight or settled). Null where it says nothing:
 * no record of the conversation, or one that cannot be read.
 */
export async function readRecord(kind, session, env) {
  const { reading, settled } = await question({ op: 'records', kind, session, env })
  if (reading.unknown) return null
  return {
    items: reading.items,
    settled,
    settlement: reading.settlement.state,
    failed: reading.failed,
    quota: reading.quota,
  }
}

/**
 * The harness's own window on a new conversation, as the app opens it:
 * `{command, args, env, dropEnv}` and, for Devin, the first message as its
 * `prompt`. `agent` is `{kind, model, effort, thinking}`; `session` is the
 * conversation's id where the harness takes one (Claude, Pi) and `seed` the first
 * message where the window takes one. Null for a harness with no window of its own,
 * and for one that needs an id when none is given.
 */
export const interactiveStart = (agent, session, seed) =>
  question({ op: 'start', agent, session, seed })

/** A message as a window can take it. */
export const windowText = (text) => question({ op: 'window_text', text })

/** A text as Windows' console carries it to a window that reads key presses. */
export const consoleText = (text) => question({ op: 'console_text', text })

/**
 * The keys that interrupt a turn in a harness's window, as its adapter says
 * them to the daemon: `{presses, closeAfterMs}`, Escape that many times in a
 * row and, where `closeAfterMs` is not null, once more that long after.
 */
export const interruptOf = (kind) => question({ op: 'interrupt', kind })

/**
 * The settings file of a Claude window's launch (`launch`, a lowercase uuid),
 * written in the ConsensFlow folder `env` names, and the arguments that load
 * it. `boardQuestions` has a member's question tool answered from the board.
 */
export const claudeSettings = (env, launch, { boardQuestions = true } = {}) =>
  question({ op: 'claude_settings', env, launch, boardQuestions })
