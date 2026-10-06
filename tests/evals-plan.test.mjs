import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  CHIEF_MODELS,
  chiefEnvironment,
  claudeProjectKey,
  codexIsolation,
  devinPickerAnswers,
  HARNESSES,
  lastLines,
  liveEnvironment,
  realOnPath,
  staffFor,
  terminalAnswer,
} from '../evals/plan.mjs'

/** The eval's plan: who is on the staff, how the chief gets its model, how the human answers. */
describe('an eval run’s plan', () => {
  it('gives each staff harness two workers, an advisor and a reviewer on its cheap model, all standard tier', () => {
    const { agents, staff } = staffFor(['codex', 'pi'], { pi: 'openrouter/other' })
    assert.deepEqual(
      agents.map((a) => [a.id, a.kind, a.model, a.workTier]),
      [
        ['eval-codex-worker', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-codex-worker-2', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-codex-advisor', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-codex-reviewer', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-pi-worker', 'pi', 'openrouter/other', 'standard'],
        ['eval-pi-worker-2', 'pi', 'openrouter/other', 'standard'],
        ['eval-pi-advisor', 'pi', 'openrouter/other', 'standard'],
        ['eval-pi-reviewer', 'pi', 'openrouter/other', 'standard'],
      ],
    )
    assert.deepEqual(
      staff.map((s) => [s.agent, s.roles[0]]),
      agents.map((a, i) => [a.id, ['worker', 'worker', 'advisor', 'reviewer'][i % 4]]),
    )
    // An effort reaches every member as its harness names it; Devin has none.
    const effort = staffFor(['claude', 'pi', 'devin'], {}, 'medium').agents
    assert.deepEqual(
      [effort[0].effort, effort[4].thinking, effort[4].effort, 'effort' in effort[8]],
      ['medium', 'medium', undefined, false],
    )
    assert.equal('effort' in agents[0], false, 'no effort unless one is given')
    assert.throws(() => staffFor(['kimi']), /no such eval harness: kimi/)
    assert.deepEqual(Object.keys(HARNESSES), ['claude', 'codex', 'pi', 'opencode', 'devin'])
  })

  it("sets the chief's model through the environment where the harness takes it there", () => {
    assert.deepEqual(chiefEnvironment('claude', 'claude-opus-5'), {
      env: { ANTHROPIC_MODEL: 'claude-opus-5' },
      model: 'claude-opus-5',
    })
    assert.deepEqual(chiefEnvironment('opencode', 'opencode/x'), {
      env: { OPENCODE_CONFIG_CONTENT: '{"model":"opencode/x"}' },
      model: 'opencode/x',
    })
    assert.deepEqual(chiefEnvironment('codex'), { env: {}, model: HARNESSES.codex.model })
    assert.deepEqual(chiefEnvironment('codex', 'gpt-x'), { env: {}, model: 'gpt-x' })
    assert.deepEqual(chiefEnvironment('pi', 'openrouter/x'), { env: {}, model: 'openrouter/x' })
    assert.deepEqual(chiefEnvironment('pi').model, CHIEF_MODELS.pi)
    assert.deepEqual(chiefEnvironment('claude').model, 'claude-opus-5')
    assert.deepEqual(chiefEnvironment('opencode').model, CHIEF_MODELS.opencode)
    // A Devin chief runs its staff's model, whatever --model says.
    assert.deepEqual(chiefEnvironment('devin', 'x'), { env: {}, model: HARNESSES.devin.model })
    assert.throws(() => chiefEnvironment('kimi', 'x'), /no such eval harness/)
  })

  it("names Claude Code's folder for a workspace as Claude does: slashes and dots become dashes", () => {
    assert.equal(
      claudeProjectKey('/Users/x/.consensflow-candidate/evals/workspace'),
      '-Users-x--consensflow-candidate-evals-workspace',
    )
  })

  it("finds the real claude or codex on PATH, never the eval wrapper or a terminal app's shim", () => {
    const present = new Set([
      '/tmp/T/cmux-cli-shims/8BB3/claude',
      '/Applications/cmux.app/Contents/Resources/bin/claude',
      '/h/.consensflow-candidate/evals/bin/claude',
      '/h/.local/bin/claude',
      '/opt/homebrew/bin/codex',
    ])
    const path =
      '/tmp/T/cmux-cli-shims/8BB3:/Applications/cmux.app/Contents/Resources/bin:/h/.consensflow-candidate/evals/bin:/h/.local/bin:/opt/homebrew/bin'
    assert.equal(
      realOnPath('claude', path, (f) => present.has(f), 'darwin'),
      '/h/.local/bin/claude',
    )
    assert.equal(
      realOnPath('codex', path, (f) => present.has(f), 'darwin'),
      '/opt/homebrew/bin/codex',
    )
    assert.throws(
      () => realOnPath('claude', '/usr/bin', () => false, 'darwin'),
      /claude is not on PATH/,
    )
    // Windows: semicolons, and the command's own extension.
    const windows = new Set(['C:\\Users\\a\\.local\\bin\\claude.exe'])
    assert.equal(
      realOnPath(
        'claude',
        'C:\\Users\\a\\.consensflow-candidate\\evals\\bin;C:\\Users\\a\\.local\\bin\\',
        (f) => windows.has(f),
        'win32',
      ),
      'C:\\Users\\a\\.local\\bin\\claude.exe',
    )
  })

  it("gives a live run's windows the user's own home, and on Windows what its programs need", () => {
    const env = {
      USER: 'u',
      PATH: 'C:\\Windows;C:\\tools',
      SystemRoot: 'C:\\Windows',
      APPDATA: 'A',
    }
    const mac = liveEnvironment({ home: '/h', bin: '/h/bin', env, platform: 'darwin' })
    assert.equal(mac.PATH.split(':')[0], '/h/bin')
    assert.ok(mac.PATH.split(':').includes('/h/.local/bin'))
    assert.equal(mac.SystemRoot, undefined)
    // Null removes the test pane host's sandbox default, so Claude keeps its own config.
    assert.equal(mac.CLAUDE_CONFIG_DIR, null)
    const windows = liveEnvironment({ home: 'C:\\Users\\u', env, platform: 'win32' })
    assert.equal(windows.PATH, 'C:\\Windows;C:\\tools')
    assert.deepEqual([windows.SystemRoot, windows.APPDATA], ['C:\\Windows', 'A'])
    assert.equal(windows.ComSpec, undefined, 'only what the machine has')
  })

  it('switches off every Codex MCP server with a harmless, disabled definition', () => {
    assert.deepEqual(codexIsolation([{ name: 'cua_repl' }, { name: 'computer-history' }]), [
      '-c',
      'mcp_servers.cua_repl.command="/usr/bin/true"',
      '-c',
      'mcp_servers.cua_repl.enabled=false',
      '-c',
      'mcp_servers.computer-history.command="/usr/bin/true"',
      '-c',
      'mcp_servers.computer-history.enabled=false',
    ])
    assert.deepEqual(codexIsolation([]), [])
    assert.throws(() => codexIsolation([{ name: 'a.b' }]), /cannot switch off/)
  })

  it('switches off a Codex MCP server reached by URL as a URL, and one reached by command as a command', () => {
    assert.deepEqual(
      codexIsolation([
        { name: 'cua_repl', transport: { type: 'stdio', command: 'cua' } },
        {
          name: 'idea',
          transport: { type: 'streamable_http', url: 'http://127.0.0.1:64342/stream' },
        },
        { name: 'computer-history' },
      ]),
      [
        '-c',
        'mcp_servers.cua_repl.command="/usr/bin/true"',
        '-c',
        'mcp_servers.cua_repl.enabled=false',
        '-c',
        'mcp_servers.idea.url="http://127.0.0.1:9/disabled"',
        '-c',
        'mcp_servers.idea.enabled=false',
        '-c',
        'mcp_servers.computer-history.command="/usr/bin/true"',
        '-c',
        'mcp_servers.computer-history.enabled=false',
      ],
    )
  })

  it('keeps the last non-empty lines a window printed, whatever the line ending', () => {
    assert.deepEqual(lastLines('a\r\n\r\nb  \rc\n\n  \nd\n', 3), ['b', 'c', 'd'])
    assert.deepEqual(lastLines(''), [])
  })
})

describe("the owner's answer in a chief's terminal", () => {
  const scenario = {
    answers: [
      { match: /document/i, text: 'Keep it, with a note on top.' },
      { match: /publish/i, text: 'Not yet.' },
    ],
    fallback: 'Yes.',
  }

  it('answers each question the chief left there, once per subject, in order', () => {
    const text = [
      'I read the files. Two things:',
      '1. Do we keep the old document in docs/?',
      '2. Should I publish the page today?',
      'Also: the reference document, do we keep it as is?',
    ].join('\n')
    assert.equal(terminalAnswer(scenario, text), 'Keep it, with a note on top. Not yet.')
  })

  it("answers a Devin chief's dialog: the scenario's words in Other, past each question's options", () => {
    const scenario = { answers: [{ match: /file|name/i, text: 'site/cariere.html' }] }
    const questions = [
      {
        question: 'Which file name?',
        header: 'File name',
        options: [{ label: 'a.html' }, { label: 'b.html' }],
      },
      {
        question: 'Which language first?',
        header: 'Language',
        options: [{ label: 'Romanian' }, { label: 'English' }, { label: 'Both' }],
      },
    ]
    const down = [27, 91, 66]
    assert.deepEqual(
      devinPickerAnswers(scenario, questions, (text) => text.toUpperCase()),
      [
        {
          question: 'Which file name?',
          answer: 'site/cariere.html',
          keys: [down, down, [13], [...Buffer.from('SITE/CARIERE.HTML')], [13]],
        },
        { question: 'Which language first?', answer: null, keys: [[13]] },
      ],
    )
  })

  it('falls back for a question it has no answer for', () => {
    assert.equal(terminalAnswer(scenario, 'Shall I start with the footer?'), 'Yes.')
    assert.equal(terminalAnswer(scenario, 'Keep the `docs/?` folder? And the footer?'), 'Yes.')
  })
})
