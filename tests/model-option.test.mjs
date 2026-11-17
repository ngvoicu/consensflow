import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { hasDistinctModelName, modelOptionText } from '../app/ui/core/model-option.js'

describe('model names and ids', () => {
  it('shows a name only when a word is not covered by the id', () => {
    const cases = [
      ['claude-opus-5-5', 'Claude Opus 5.5', false],
      ['claude-haiku-4-5', 'Claude Haiku 4.5 (latest)', true],
      ['mistral-small-2603', 'Mistral Small 4', true],
      // A single character inside a different id word does not cover it either.
      ['mistral-small-2603', 'Mistral Small 6', true],
      ['gpt-o-1', 'GPT O1', false],
      ['anthropic/claude-fable-5', 'Anthropic: Claude Fable 5', false],
      ['gpt-6-sol-priority', 'GPT-6 Sol Fast', true],
      ['módel-12', 'Módel 12', false],
      ['módel-12', 'Módel 12 Édition', true],
      ['𐐨𐐩', '𐐨', true],
      ['model-123', '', false],
    ]
    for (const [id, name, distinct] of cases) {
      assert.equal(hasDistinctModelName(id, name), distinct, `${id}: ${name}`)
    }
  })

  it('orders the id, distinct name, provider and harness default mark', () => {
    assert.equal(
      modelOptionText({
        id: 'claude-haiku-4-5',
        label: 'Claude Haiku 4.5 (latest)',
        provider: 'anthropic',
        markedDefault: true,
      }),
      'claude-haiku-4-5 (Claude Haiku 4.5 (latest)) [anthropic] · default',
    )
    assert.equal(modelOptionText({ id: 'swe-2', label: 'SWE-2' }), 'swe-2')
    assert.equal(modelOptionText({ id: 'model-id', label: '' }), 'model-id')
  })
})
