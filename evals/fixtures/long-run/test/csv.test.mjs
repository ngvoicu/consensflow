import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { parse, stringify } from '../src/csv.mjs'

describe('csv', () => {
  it('reads the rows of a file, each by the names of its header', async () => {
    await pace()
    const rows = parse('code,name,kind\n1000,Bank,asset\n6100,Office supplies,expense\n')
    assert.deepEqual(rows, [
      { code: '1000', name: 'Bank', kind: 'asset' },
      { code: '6100', name: 'Office supplies', kind: 'expense' },
    ])
  })

  it('skips blank lines and takes a missing field as empty', async () => {
    await pace()
    assert.deepEqual(parse('a,b\n\n1\r\n'), [{ a: '1', b: '' }])
    assert.deepEqual(parse(''), [])
  })

  it('writes a field in quotes where it holds a comma or a quote', async () => {
    await pace()
    const text = stringify([{ memo: 'Office, supplies', note: 'say "hi"', n: 3 }], ['memo', 'note', 'n'])
    assert.equal(text, 'memo,note,n\n"Office, supplies","say ""hi""",3\n')
  })
})
