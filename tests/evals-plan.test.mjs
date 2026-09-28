import assert from 'node:assert/strict'
import { mkdtemp, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { describe, it } from 'node:test'
import {
  answerFor,
  chiefEnvironment,
  claudeProjectKey,
  codexIsolation,
  HARNESSES,
  lastLines,
  realOnPath,
  staffFor,
} from '../evals/plan.mjs'
import sixDecisions from '../evals/scenarios/six-decisions.mjs'
import { openLedger } from '../src/ledger/index.js'

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
    assert.deepEqual(chiefEnvironment('pi', 'ignored'), { env: {}, model: "pi's default" })
    assert.deepEqual(chiefEnvironment('claude').model, 'claude-opus-5')
    assert.deepEqual(chiefEnvironment('opencode').model, HARNESSES.opencode.model)
    assert.throws(() => chiefEnvironment('kimi', 'x'), /no such eval harness/)
  })

  it("names Claude Code's folder for a workspace as Claude does: slashes and dots become dashes", () => {
    assert.equal(
      claudeProjectKey('/Users/x/.consensflow-candidate/evals/workspace'),
      '-Users-x--consensflow-candidate-evals-workspace',
    )
  })

  it('finds the real claude or codex on PATH, never the eval wrapper', () => {
    const present = new Set([
      '/h/.consensflow-candidate/evals/bin/claude',
      '/h/.local/bin/claude',
      '/opt/homebrew/bin/codex',
    ])
    const path = '/h/.consensflow-candidate/evals/bin:/h/.local/bin:/opt/homebrew/bin'
    assert.equal(
      realOnPath('claude', path, (f) => present.has(f)),
      '/h/.local/bin/claude',
    )
    assert.equal(
      realOnPath('codex', path, (f) => present.has(f)),
      '/opt/homebrew/bin/codex',
    )
    assert.throws(() => realOnPath('claude', '/usr/bin', () => false), /claude is not on PATH/)
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

  it('keeps the last non-empty lines a window printed, whatever the line ending', () => {
    assert.deepEqual(lastLines('a\r\n\r\nb  \rc\n\n  \nd\n', 3), ['b', 'c', 'd'])
    assert.deepEqual(lastLines(''), [])
  })

  it('answers as the board does: a body for a plain question, one pick per sub-question for options', () => {
    const scenario = {
      answers: [
        { match: /publish/i, text: 'Not yet; after I see it.' },
        { match: /recommend/i, text: 'Yes, as you recommend.' },
      ],
      fallback: 'Yes.',
    }
    assert.deepEqual(answerFor(scenario, { body: 'Shall we publish now?', questions: null }), {
      body: 'Not yet; after I see it.',
    })
    assert.deepEqual(answerFor(scenario, { body: 'Keep the old document?', questions: null }), {
      body: 'Yes.',
    })
    assert.deepEqual(
      answerFor(scenario, { body: 'Recommend an order?\nWe publish after that.', questions: null }),
      { body: 'Yes, as you recommend.' },
      'the first line, the subject, wins over a word further down',
    )
    assert.deepEqual(
      answerFor(scenario, {
        body: 'Three things to settle',
        questions: [
          { header: 'Publish', question: 'Publish now?', options: [{ label: 'Yes' }] },
          { header: 'Order', question: 'Which order?', options: [{ label: 'Law first' }] },
          { header: 'Tone', question: 'Do you recommend a formal tone?', options: [] },
        ],
      }),
      { choices: [['Not yet; after I see it.'], ['Law first'], ['Yes, as you recommend.']] },
      'free text where a pattern matches, else the first option, one pick each',
    )
  })

  it('gives an answer the ledger takes, for a question with several sub-questions', async () => {
    const dir = await mkdtemp(path.join(os.tmpdir(), 'cf-evals-answer-'))
    try {
      const ledger = openLedger(path.join(dir, 'consensflow.db'))
      const project = ledger.createProject({
        directory: '/work/site',
        name: 'site',
        chief: { harness: 'opencode' },
      })
      const questions = [
        {
          question: 'Ce facem cu documentul de referință HR din docs/?',
          header: 'HR document',
          options: [{ label: 'Keep it' }, { label: 'Delete it' }],
        },
        {
          question: 'When do we publish?',
          header: 'Publish',
          options: [{ label: 'Now' }, { label: 'Later' }],
        },
      ]
      const asked = ledger.ask(project.id, {
        from: 'chief',
        to: 'human',
        body: 'HR document: Ce facem cu documentul de referință HR din docs/?',
        questions,
      })
      const answer = answerFor(sixDecisions, asked)
      const stored = ledger.answer(asked.id, { from: 'human', ...answer })
      assert.equal(
        stored.body,
        'HR document: Îl păstrăm, dar pune sus o notă că pagina de legislație e sursa actuală.\nPublish: Nu publicăm încă. Vreau să văd pagina întâi; îți spun eu când.',
      )
      ledger.close()
    } finally {
      await rm(dir, { recursive: true, force: true })
    }
  })
})
