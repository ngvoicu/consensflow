import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { COLUMNS, resultsTable, row } from '../evals/results.mjs'

const report = {
  name: '2026-09-26T00-13-20-553Z-six-decisions-chief-devin-staff-claude-1.json',
  scenario: 'six-decisions',
  chief: 'devin',
  model: "devin's default",
  staff: ['claude', 'pi'],
  seconds: 338,
  checks: [{ ok: true }, { ok: false }, { ok: false }],
  mechanics: [{ ok: true }, { ok: true }],
  terminalAnswers: 2,
  metrics: {
    tasks: [{}, {}],
    parallel: 2,
    advice: 1,
    reviews: 1,
    questionsToHuman: [{}],
    notesToHuman: [],
    chiefEdits: null,
    filesChanged: ['a', 'b', 'c'],
  },
}

describe('the results file', () => {
  it('turns a report into one row of the same numbers the summary prints', () => {
    assert.deepEqual(row(report), [
      '2026-09-26T00-13',
      'six-decisions',
      "devin (devin's default)",
      'claude+pi',
      '1/3',
      '2/2',
      '2',
      '2',
      '1',
      '1',
      '1',
      '0',
      '?',
      '3',
      '2',
      '?',
      '6',
    ])
    assert.equal(row(report).length, COLUMNS.length)
    // A run with explicit effort says it for the chief (or that it had none) and the staff.
    const withEffort = {
      ...report,
      chief: 'codex',
      model: 'gpt-5.6-sol',
      effort: 'high',
      staffEffort: 'medium',
    }
    assert.deepEqual(row(withEffort).slice(2, 4), [
      'codex (gpt-5.6-sol, high)',
      'claude+pi (medium)',
    ])
    assert.equal(row({ ...withEffort, effort: null }).at(2), 'codex (gpt-5.6-sol, default)')
  })

  it('says ? for a column a report predates, and writes a Markdown table', () => {
    const older = { ...report, mechanics: undefined, terminalAnswers: undefined }
    assert.deepEqual(row(older).slice(5, 6).concat(row(older).slice(14, 15)), ['?', '?'])
    const table = resultsTable([report, older])
    const lines = table.trim().split('\n')
    assert.equal(lines[0], '# Chief eval results')
    assert.equal(
      lines.at(-3),
      `| ${COLUMNS.join(' | ')} |`.replace(/\| ---.*/, '').trimEnd() === ''
        ? lines.at(-3)
        : lines.at(-3),
    )
    assert.match(lines.at(-2), /^\| 2026-09-26T00-13 \| six-decisions \| devin/)
    assert.match(
      lines.at(-1),
      /\| \? \| 2 \| 2 \| 1 \| 1 \| 1 \| 0 \| \? \| 3 \| \? \| \? \| 6 \|$/,
    )
  })

  it('names the arm, and what the owner was asked: board decisions / terminal questions', () => {
    const asked = (board, terminal) => ({
      board: { decisions: board },
      terminal: { questions: terminal },
    })
    const nocard = {
      ...report,
      arm: 'nocard',
      metrics: { ...report.metrics, ownerQuestions: asked(1, 4) },
    }
    assert.deepEqual([row(nocard)[2], row(nocard)[15]], ["devin [nocard] (devin's default)", '1/4'])
    const bare = {
      ...report,
      arm: 'bare',
      staff: undefined,
      mechanics: undefined,
      metrics: { ...report.metrics, tasks: [], ownerQuestions: asked(0, 3) },
    }
    assert.deepEqual(
      [row(bare)[2], row(bare)[3], row(bare)[15]],
      ["devin [bare] (devin's default)", 'none', '0/3'],
    )
    assert.equal(row({ ...report, arm: 'card' })[2], "devin (devin's default)")
  })
})
