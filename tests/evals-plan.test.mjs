import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { answerFor, chiefEnvironment, HARNESSES, staffFor } from '../evals/plan.mjs'

/** The eval's plan: who is on the staff, how the chief gets its model, how the human answers. */
describe('an eval run’s plan', () => {
  it('gives each staff harness two workers, an advisor and a reviewer on its cheap model, all standard tier', () => {
    const { agents, staff } = staffFor(['codex', 'pi'], { pi: 'opencode-go/other' })
    assert.deepEqual(
      agents.map((a) => [a.id, a.kind, a.model, a.workTier]),
      [
        ['eval-codex-worker', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-codex-worker-2', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-codex-advisor', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-codex-reviewer', 'codex', 'gpt-5.6-luna', 'standard'],
        ['eval-pi-worker', 'pi', 'opencode-go/other', 'standard'],
        ['eval-pi-worker-2', 'pi', 'opencode-go/other', 'standard'],
        ['eval-pi-advisor', 'pi', 'opencode-go/other', 'standard'],
        ['eval-pi-reviewer', 'pi', 'opencode-go/other', 'standard'],
      ],
    )
    assert.deepEqual(
      staff.map((s) => [s.agent, s.roles[0]]),
      agents.map((a, i) => [a.id, ['worker', 'worker', 'advisor', 'reviewer'][i % 4]]),
    )
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
    assert.deepEqual(chiefEnvironment('codex', 'ignored'), { env: {}, model: "codex's default" })
    assert.throws(() => chiefEnvironment('kimi', 'x'), /no such eval harness/)
  })

  it('answers by the scenario’s patterns first, free text included, then by options, then the fallback', () => {
    const scenario = {
      answers: [
        { match: /publish/i, text: 'Not yet; after I see it.' },
        { match: /recommend/i, text: 'Yes, as you recommend.' },
      ],
      fallback: 'Yes.',
    }
    assert.equal(
      answerFor(scenario, { body: 'Shall we publish now?', questions: null }),
      'Not yet; after I see it.',
    )
    assert.equal(
      answerFor(scenario, {
        body: 'Which order?',
        questions: [
          { question: 'Order', options: [{ label: 'Law first' }, { label: 'Guide first' }] },
        ],
      }),
      'Law first',
    )
    assert.equal(answerFor(scenario, { body: 'Keep the old document?', questions: null }), 'Yes.')
    assert.equal(
      answerFor(scenario, {
        body: 'I recommend a note. Publish?',
        questions: [{ question: 'x', options: [{ label: 'A' }] }],
      }),
      'Not yet; after I see it.',
      'a pattern wins over options',
    )
  })
})
