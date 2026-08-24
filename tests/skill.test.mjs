import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { generateSkill } from '../src/skill.js'

const ROSTER = [
  {
    name: 'zeus',
    harness: 'claude',
    model: 'claude-opus-5',
    effort: 'max',
    description: 'Deepest reviewer.',
  },
  {
    name: 'hyperion',
    harness: 'codex',
    model: 'gpt-5.6-sol',
    effort: 'ultra',
  },
  {
    name: 'endymion',
    harness: 'pi',
    model: 'openrouter/moonshotai/kimi-k3',
    effort: 'xhigh',
  },
  {
    name: 'mani',
    harness: 'opencode',
    model: 'openrouter/moonshotai/kimi-k3',
  },
  {
    name: 'loki',
    harness: 'codex',
    model: 'gpt-5.6-luna',
    effort: 'xhigh',
  },
]

describe('one command builder serves the skill and the roster editor', () => {
  const md = generateSkill(ROSTER)

  it('gives the exact line the skill table will contain', async () => {
    const { agentCommand } = await import('../src/skill.js')
    const command = agentCommand(ROSTER[0])
    assert.equal(command, `cf run @${ROSTER[0].name} "<task>"`)
    for (const agent of ROSTER) {
      assert.equal(agentCommand(agent), `cf run @${agent.name} "<task>"`)
    }
  })

  it('lists every agent name in the description, so mentions trigger it', () => {
    const description = md.slice(0, md.indexOf('\n---\n'))
    for (const p of ROSTER) assert.ok(description.includes(p.name), `${p.name} missing`)
  })

  it('tells the harness to consult on its own initiative, not only when asked', () => {
    // The roster is worth nothing if the lead only consults when told to.
    assert.match(md, /Reach for an advisor on your own/)
    assert.match(md, /do not need permission to consult/i)
    assert.match(md, /riskiest assumption/i)
    // And it says the one thing this mode cannot do: see the conversation.
    assert.match(md, /cannot see this conversation/i)
  })

  it('teaches the harness to self-heal when a name is missing from the table', () => {
    assert.match(md, /cf skills update/)
    assert.match(md, /missing from (the|this) table/i)
  })

  it('keeps the etiquette that survived two product generations', () => {
    assert.match(md, /One agent at a time/i)
    assert.match(md, /Never apply an agent/i)
    assert.match(md, /never edit this file by hand/i)
  })

  it('refuses to generate from an empty roster rather than shipping a blank skill', () => {
    assert.throws(() => generateSkill([]), /empty roster/i)
  })
})

describe('the skill teaches one verb, with the flags that make a spawn', () => {
  const md = generateSkill([
    { name: 'zeus', harness: 'claude', model: 'claude-opus-5', effort: 'max' },
    { name: 'pygmalion', harness: 'image', model: 'gpt-image-2' },
  ])

  it('spawns every agent the same way, image agents included', () => {
    assert.match(md, /cf run @<name> "<task>"/)
    assert.match(md, /pygmalion/)
    assert.match(md, /gpt-image-2/)
    assert.doesNotMatch(md, /codex exec/)
    assert.doesNotMatch(md, /env -u ANTHROPIC_API_KEY/)
  })

  it('makes the lead stop and discuss what came back', () => {
    // The failure this prevents: an agent answers, the lead quietly acts on it,
    // and the user never hears the second opinion they paid for.
    assert.match(md, /Bring the answer back before anything else/)
    assert.match(md, /do not\s+spawn a second agent/i)
    assert.match(md, /not a decision/i)
  })

  it('says the brief and the handoff are the lead to give', () => {
    assert.match(md, /--brief/)
    assert.match(md, /--handoff-file/)
    assert.match(md, /You are the one holding it/)
  })
})

describe('cmux mode spawns into a pane; the host modes are untouched', () => {
  const roster = [
    { name: 'athena', harness: 'pi', model: 'openrouter/qwen/qwen3.8-27b', effort: 'max' },
  ]

  // Superseded by spec cmux-agent-threads [TEST-THR-15]: a pane is now one
  // CONVERSATION, not one agent, and threading makes the old "a window, not a
  // memory" caveat false in cmux mode. Both assertions moved to the
  // conversation describe block below rather than being softened here.

  it('says nothing about panes in claude or pi mode', () => {
    for (const mode of ['claude', 'pi']) {
      const skill = generateSkill(roster, { mode })
      assert.doesNotMatch(skill, /pane/i, `${mode} mode must not mention panes`)
    }
  })

  it('says nothing about panes when no mode is given', () => {
    assert.doesNotMatch(generateSkill(roster), /pane/i)
  })

  it('still names the roster in every mode', () => {
    for (const mode of ['claude', 'pi', 'cmux']) {
      assert.match(generateSkill(roster, { mode }), /named AI agents — athena/)
    }
  })
})

describe('cmux mode teaches conversations, one pane each', () => {
  const roster = [
    { name: 'ares', harness: 'pi', model: 'openrouter/x-ai/grok-4.6', effort: 'high' },
  ]

  it('hands the lead the pane commands instead of making it explore', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    // A lead without these spent a minute dumping `cmux --help` twice before
    // it could ask a question. The recipe is three commands; quote them.
    assert.match(cmux, /cmux new-pane --type terminal --direction right --focus false/)
    assert.match(cmux, /cmux send --surface surface:NN/)
    // A new pane does not inherit the lead's directory, and conversations are
    // keyed by directory — without the cd, the consult lands in a namespace
    // the lead cannot read back with `cf last`.
    assert.match(cmux, /cd "'"\$PWD"'"/, 'the send must cd first')
    assert.match(cmux, /cmux rename-tab --surface surface:NN/)
  })

  it('tells the lead to read the answer, not scrape the screen', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    // Reading screen text back is the typed-bootstrap minefield v3 was built
    // to avoid — and `cf last` exists precisely so nobody has to.
    assert.match(cmux, /cf last <name>/)
    assert.match(cmux, /do not\s+scrape/i)
  })

  it('still says the commands belong to cmux, not to us', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    assert.match(cmux, /they\s+are cmux's, not ours/i)
  })

  it('says one pane per conversation, not per agent', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    assert.match(cmux, /one pane per conversation/i)
    // Named conversations are why an agent can have more than one pane, which
    // is exactly what the pane-per-agent wording could not express.
    assert.match(cmux, /--new/)
    assert.match(cmux, /--session/)
  })

  it('teaches that a follow-up continues rather than restarts', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    assert.match(cmux, /continues/i)
    assert.doesNotMatch(
      cmux,
      /a window, not a memory/i,
      'that caveat described the un-threaded world; threading makes it false here',
    )
  })

  it('leaves the host modes one-shot and pane-free', () => {
    for (const mode of ['claude', 'pi']) {
      const skill = generateSkill(roster, { mode })
      assert.doesNotMatch(skill, /pane/i, `${mode} must not mention panes`)
      assert.doesNotMatch(skill, /--session/, `${mode} is one-shot`)
    }
  })
})

describe('the description is all a lead reads before it decides to look inside', () => {
  const description = (skill) => skill.slice(0, skill.indexOf('\n---\n'))
  const roster = ROSTER

  it('does not call a cmux consult one-shot, because it is not one', () => {
    // The transcript that started this (2026-08-24): a lead read "run one-shot
    // in the current directory", concluded there was nothing further to learn,
    // and ran the consult in its own pane without ever opening the body. The
    // description is the only text it sees before that decision, so a sentence
    // that sounds complete is a sentence that keeps the body shut.
    const d = description(generateSkill(roster, { mode: 'cmux' }))

    assert.doesNotMatch(d, /one-shot/i)
    assert.match(d, /conversation/i)
    assert.match(d, /pane/i)
  })

  it('pulls the lead into the body rather than standing in for it', () => {
    const d = description(generateSkill(roster, { mode: 'cmux' }))

    assert.match(d, /this skill/i, 'it says to read the skill')
    // A runnable command here is an invitation to skip everything after it.
    assert.doesNotMatch(d, /cf run/)
  })

  it('still says one-shot in the host modes, where it is true', () => {
    for (const mode of ['claude', 'pi']) {
      const d = description(generateSkill(roster, { mode }))
      assert.match(d, /one-shot/i, `${mode} really is one-shot`)
      assert.doesNotMatch(d, /pane/i)
    }
  })

  it('does not tell a cmux lead in the body either that its run is one-shot', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    assert.doesNotMatch(cmux, /run one-shot by its own harness/)
  })

  it('gives a cmux lead no bare consult command to copy out of the steps', () => {
    // Step 3 used to be a fenced `cf run @<name> "<task>"` with the pane rule
    // as prose underneath it. A lead skimming for the thing to run copies the
    // fence and never reaches the sentence.
    const cmux = generateSkill(roster, { mode: 'cmux' })

    const fenced = /```bash\n\s*cf run @<name> "<task>"\n\s*```/
    assert.doesNotMatch(cmux, fenced)
    // The host modes keep it: there is no pane to send it to.
    assert.match(generateSkill(roster, { mode: 'claude' }), fenced)
  })

  it('tells a cmux lead that the conversations here are not all its own', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    assert.match(cmux, /belongs to the session that started it/i)
  })

  it('names the conversation before the pane, with cf mint', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    // The run prints its conversation name into a pane the lead cannot read;
    // minting first is what makes the tab title and the read-back possible.
    assert.match(cmux, /NAME=\$\(cf mint\)/)
    assert.match(cmux, /--new --session /)
    // And finding the pane again is a quoted command, not an exploration.
    assert.match(cmux, /cmux list-pane-surfaces/)
  })

  it('teaches that the pane becomes the agent own window, not a stream', () => {
    const cmux = generateSkill(roster, { mode: 'cmux' })

    assert.match(cmux, /the pane IS the agent's window/i)
    assert.match(cmux, /codex/, 'the one exception is named')
    // Follow-ups are typed into the window, not wrapped in a command.
    assert.match(cmux, /plain text/)
    // And the read is the harness's own store, waited on.
    assert.match(cmux, /cf catchup <name> --wait/)
  })
})
