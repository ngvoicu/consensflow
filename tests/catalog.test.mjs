import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { CATALOG, catalogEntry, EFFORTS } from '../src/catalog.js'
import { HARNESSES } from '../src/roster.js'

describe('every tool ships a list of ready-made agents', () => {
  it('covers every harness, each with a real list (image has one)', () => {
    assert.deepEqual(Object.keys(CATALOG).sort(), [...HARNESSES].sort())
    for (const [harness, entries] of Object.entries(CATALOG)) {
      const least = harness === 'image' ? 1 : 3
      assert.ok(entries.length >= least, `${harness} needs a real list`)
    }
  })

  it('gives every entry a name, a model and a description', () => {
    for (const entries of Object.values(CATALOG)) {
      for (const entry of entries) {
        assert.match(entry.name, /^[a-z][a-z0-9-]*$/)
        assert.ok(entry.model.length > 0)
        assert.ok(entry.description.length > 0)
      }
    }
  })

  it('never repeats a name across the whole catalog', () => {
    const names = Object.values(CATALOG).flatMap((entries) => entries.map((e) => e.name))
    assert.equal(new Set(names).size, names.length)
  })

  it('uses only efforts its harness actually accepts', () => {
    for (const [harness, entries] of Object.entries(CATALOG)) {
      for (const entry of entries) {
        if (entry.effort === undefined) continue
        assert.ok(
          EFFORTS[harness].includes(entry.effort),
          `${entry.name}: ${harness} has no effort ${entry.effort}`,
        )
      }
    }
  })

  it('carries the models verified live on 2026-08-21, newest of each family', () => {
    const models = Object.values(CATALOG).flatMap((entries) => entries.map((e) => e.model))
    assert.ok(models.includes('openrouter/z-ai/glm-5.3'))
    assert.ok(models.includes('openrouter/qwen/qwen3.8-max'))
    assert.ok(models.includes('openrouter/moonshotai/kimi-k3'))
    // Added 2026-08-24, each confirmed present in `pi --list-models` and
    // `opencode models` before it was written down — two free tiers and one
    // unbadged stealth model, on both open-model harnesses.
    for (const model of [
      'openrouter/stealth/ox-alpha',
      'openrouter/nvidia/nemotron-3-ultra-550b-a55b:free',
      'openrouter/poolside/laguna-s-2.1:free',
    ]) {
      assert.ok(
        CATALOG.pi.some((e) => e.model === model),
        `pi is missing ${model}`,
      )
      assert.ok(
        CATALOG.opencode.some((e) => e.model === model),
        `opencode is missing ${model}`,
      )
    }
    // Superseded versions must not linger in a curated list.
    assert.ok(!models.some((m) => m.includes('glm-5.2')))
    assert.ok(!models.some((m) => m.includes('qwen3.7')))
  })

  it('is the payload presets and nothing else — one list, not two', async () => {
    // A second hand-written list is how `nike` came to mean GPT-5.6-luna in the
    // app and Gemini 3.7 Flash in the harness. The catalog is now derived, so
    // the two can no longer disagree.
    const { AGENT_PRESETS } = await import('../hosts/lib/presets.js')
    const byName = new Map(AGENT_PRESETS.map((preset) => [preset.preset, preset]))

    for (const [harness, entries] of Object.entries(CATALOG)) {
      for (const entry of entries) {
        const preset = byName.get(entry.name)
        assert.ok(preset !== undefined, `${entry.name} exists as a preset`)
        assert.equal(entry.model, preset.model, `${entry.name}: model matches the harness`)
        assert.equal(entry.effort, preset.effort ?? preset.thinking, `${entry.name}: effort`)
        assert.equal(entry.preset, entry.name, `${entry.name}: records its provenance`)
        assert.ok(HARNESSES.includes(harness))
      }
    }

    // Every preset the manager can actually create is offered; the image
    // preset is not, because the roster has no harness that launches it.
    const offered = Object.values(CATALOG).flat().length
    assert.equal(offered, AGENT_PRESETS.length, 'every preset is offered, image included')
  })

  it('finds an entry by name, whatever tool it belongs to', () => {
    const entry = catalogEntry('zeus')
    assert.equal(entry.harness, 'claude')
    assert.equal(entry.model, 'claude-opus-5')
    assert.equal(catalogEntry('nobody'), undefined)
  })

  it('records the effort levels each CLI accepts, as its own help states them', () => {
    assert.deepEqual(EFFORTS.claude, ['low', 'medium', 'high', 'xhigh', 'max'])
    assert.ok(EFFORTS.codex.includes('ultra'))
    assert.ok(EFFORTS.pi.includes('off'))
  })
})
