/**
 * The folders the scenarios start from: an agents file as the CLI writes
 * one, and as a hand leaves one, the stand-ins for the harnesses' CLIs on the
 * PATH, and the launcher a `cf setup` leaves. A text may name `$ROOT`, `$NODE`
 * and `$REPO`, which the world puts as this machine's.
 */

/** When the agents of a fixture were made: before the clock the CLI reads. */
export const STAMP = '2026-01-01T00:00:00.000Z'

const WINDOWS = process.platform === 'win32'

export const file = (path, text, executable = false) => ({
  path,
  text,
  ...(executable ? { executable } : {}),
})

export const dir = (path) => ({ dir: path })

/** One of the human's agents as the file keeps it, in the order the CLI writes its fields. */
export function row(id, kind, model, { designer, ...rest } = {}) {
  return {
    id,
    name: id.charAt(0).toUpperCase() + id.slice(1),
    kind,
    ...(designer ? { designer } : {}),
    createdAt: STAMP,
    updatedAt: STAMP,
    model,
    ...rest,
  }
}

/** The agents file as the CLI writes it: two spaces, and a line break after. */
export function roster(agents, more = {}, home = 'consensflow') {
  return file(
    `${home}/agents.json`,
    `${JSON.stringify({ schemaVersion: 1, agents, ...more }, null, 2)}\n`,
  )
}

/** An agents file as a hand leaves it: whatever text. */
export const handwritten = (text, home = 'consensflow') => file(`${home}/agents.json`, text)

/** The human's own agents, one of each harness, and an image agent. */
export const OWN = [
  row('mine', 'claude-code', 'claude-opus-5', { effort: 'max' }),
  row('lunar', 'codex', 'gpt-5.6-luna', {
    effort: 'xhigh',
    workTier: 'standard',
    description: 'Fast and cheap',
  }),
  row('painter', 'codex', 'codex-image', { designer: true }),
  row('pilot', 'pi', 'openai-codex/gpt-6.1-sol', { thinking: 'high' }),
  row('opener', 'opencode', 'openrouter/z-ai/glm-5.3'),
  row('devon', 'devin', 'claude-fable-5-1', { effort: 'max', description: 'Debugging' }),
]

/** An agents file with the human's own agents in it. */
export const withOwn = () => roster(OWN)

/** A stand-in for a harness's CLI on the PATH, found and never run. */
export function stub(name) {
  return WINDOWS
    ? file(`bin/${name}.cmd`, '@echo off\r\nexit /b 0\r\n')
    : file(`bin/${name}`, '#!/bin/sh\nexit 0\n', true)
}

/** The five harnesses' stand-ins. */
export const ALL_STUBS = ['claude', 'codex', 'devin', 'opencode', 'pi'].map(stub)

/**
 * The launchers `cf setup` writes, one a name, naming `runtime` and the CLI of
 * this checkout. `doctor` looks at the first name alone.
 */
export function launchers(runtime = '$NODE', { pin = true, names = ['consensflow', 'cf'] } = {}) {
  return names.map((name) =>
    WINDOWS
      ? file(
          `consensflow/bin/${name}.cmd`,
          `@echo off\r\nREM Installed by ConsensFlow. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\n${pin ? 'setlocal\r\nset "CONSENSFLOW_HOME=$ROOT/consensflow"\r\n' : ''}"${runtime}" "$REPO/bin/cf.mjs" %*\r\n`,
        )
      : file(
          `consensflow/bin/${name}`,
          `#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own runtime and its own copy of\n# the CLI, so the terminal and the window never drift apart.\n${pin ? 'export CONSENSFLOW_HOME="$ROOT/consensflow"\n' : ''}exec "${runtime}" "$REPO/bin/cf.mjs" "$@"\n`,
          true,
        ),
  )
}
