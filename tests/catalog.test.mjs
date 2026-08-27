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

  // An entry with no effort draws a bare harness tag in the roster UI, so a
  // level nobody chose looks exactly like a level the catalog forgot. Where the
  // harness HAS levels, every entry names one — unless it is listed here, and
  // the file says why beside it.
  it('names an effort wherever its harness has one, or is a listed exception', () => {
    // The three models that take no effort parameter at all — see "Effort ceilings"
    // in hosts/lib/presets.js for how that was established.
    const blankOnPurpose = new Set([
      'metis', // MiniMax M3, on pi
      'mimir', // MiniMax M3, on opencode
      'triton', // Laguna S 2.1 free, on pi
      'aegir', // Laguna S 2.1 free, on opencode
    ])
    for (const [harness, entries] of Object.entries(CATALOG)) {
      if ((EFFORTS[harness] ?? []).length === 0) continue
      for (const entry of entries) {
        assert.ok(
          entry.effort !== undefined || blankOnPurpose.has(entry.name),
          `${entry.name}: ${harness} has effort levels, so this one must name one`,
        )
      }
    }
  })

  // The zoo is the same models through OpenCode: a pair that shares a model
  // must share the level too, or one name means two different agents.
  it("keeps each opencode twin at its pi twin's effort", () => {
    const level = (entry) => entry.effort
    for (const entry of CATALOG.opencode) {
      const twin = CATALOG.pi.find((p) => p.model === entry.model)
      if (twin === undefined || level(entry) === undefined) continue
      assert.equal(
        level(entry),
        level(twin),
        `${entry.name} and ${twin.name} share ${entry.model} but not the effort`,
      )
    }
  })

  it('carries the models verified live on 2026-08-21, newest of each family', () => {
    const models = Object.values(CATALOG).flatMap((entries) => entries.map((e) => e.model))
    assert.ok(models.includes('openrouter/z-ai/glm-5.3'))
    assert.ok(models.includes('openrouter/qwen/qwen3.8-max'))
    assert.ok(models.includes('openrouter/moonshotai/kimi-k3'))
    // Added 2026-08-24, each confirmed present in `pi --list-models` and
    // `opencode models` before it was written down — two free tiers and one
    // unbadged stealth model, on both open-model harnesses. The stealth one
    // ended its testing period on 2026-08-27 (404 naming its own model), so
    // nyx and nott moved to it under its real name: z-ai/glm-5.3-flash,
    // verified that day in OpenRouter's /api/v1/models and by a live one-shot
    // on each CLI — neither harness catalog lists it yet, both run it.
    for (const model of [
      'openrouter/z-ai/glm-5.3-flash',
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
    // Superseded versions must not linger in a curated list — and a retired
    // endpoint is superseded twice over: stealth/ox-alpha answers 404 now.
    assert.ok(!models.some((m) => m.includes('glm-5.2')))
    assert.ok(!models.some((m) => m.includes('qwen3.7')))
    assert.ok(!models.some((m) => m.includes('ox-alpha')))
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
