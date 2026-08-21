import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { CATALOG, catalogEntry, EFFORTS } from '../src/catalog.js'
import { RUNTIMES } from '../src/roster.js'

describe('every tool ships a list of ready-made participants', () => {
  it('covers all four runtimes, each with several entries', () => {
    assert.deepEqual(Object.keys(CATALOG).sort(), [...RUNTIMES].sort())
    for (const [runtime, entries] of Object.entries(CATALOG)) {
      assert.ok(entries.length >= 3, `${runtime} needs a real list`)
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

  it('uses only efforts its runtime actually accepts', () => {
    for (const [runtime, entries] of Object.entries(CATALOG)) {
      for (const entry of entries) {
        if (entry.effort === undefined) continue
        assert.ok(
          EFFORTS[runtime].includes(entry.effort),
          `${entry.name}: ${runtime} has no effort ${entry.effort}`,
        )
      }
    }
  })

  it('carries the models verified live on 2026-08-21, newest of each family', () => {
    const models = Object.values(CATALOG).flatMap((entries) => entries.map((e) => e.model))
    assert.ok(models.includes('openrouter/z-ai/glm-5.3'))
    assert.ok(models.includes('openrouter/qwen/qwen3.8-max'))
    assert.ok(models.includes('openrouter/moonshotai/kimi-k3'))
    // Superseded versions must not linger in a curated list.
    assert.ok(!models.some((m) => m.includes('glm-5.2')))
    assert.ok(!models.some((m) => m.includes('qwen3.7')))
  })

  it('is the payload presets and nothing else — one list, not two', async () => {
    // A second hand-written list is how `nike` came to mean GPT-5.6-luna in the
    // app and Gemini 3.7 Flash in the runtime. The catalog is now derived, so
    // the two can no longer disagree.
    const { PARTICIPANT_PRESETS } = await import('../hosts/lib/presets.js')
    const byName = new Map(PARTICIPANT_PRESETS.map((preset) => [preset.preset, preset]))

    for (const [runtime, entries] of Object.entries(CATALOG)) {
      for (const entry of entries) {
        const preset = byName.get(entry.name)
        assert.ok(preset !== undefined, `${entry.name} exists as a preset`)
        assert.equal(entry.model, preset.model, `${entry.name}: model matches the runtime`)
        assert.equal(entry.effort, preset.effort ?? preset.thinking, `${entry.name}: effort`)
        assert.equal(entry.preset, entry.name, `${entry.name}: records its provenance`)
        assert.ok(RUNTIMES.includes(runtime))
      }
    }

    // Every preset the manager can actually create is offered; the image
    // preset is not, because the roster has no runtime that launches it.
    const offered = Object.values(CATALOG).flat().length
    const launchable = PARTICIPANT_PRESETS.filter((preset) => preset.kind !== 'image').length
    assert.equal(offered, launchable, 'every launchable preset is offered')
  })

  it('finds an entry by name, whatever tool it belongs to', () => {
    const entry = catalogEntry('zeus')
    assert.equal(entry.runtime, 'claude')
    assert.equal(entry.model, 'claude-opus-5')
    assert.equal(catalogEntry('nobody'), undefined)
  })

  it('records the effort levels each CLI accepts, as its own help states them', () => {
    assert.deepEqual(EFFORTS.claude, ['low', 'medium', 'high', 'xhigh', 'max'])
    assert.ok(EFFORTS.codex.includes('ultra'))
    assert.ok(EFFORTS.pi.includes('off'))
  })
})
