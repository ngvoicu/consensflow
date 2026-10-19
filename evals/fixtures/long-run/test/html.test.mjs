import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { pace } from '../scripts/pace.mjs'
import { escape, links, page, table } from '../src/html.mjs'

describe('html', () => {
  it('escapes what HTML would read as its own', async () => {
    await pace()
    assert.equal(escape('<a href="x">R&D</a>'), '&lt;a href=&quot;x&quot;&gt;R&amp;D&lt;/a&gt;')
  })

  it('makes a page with its title, a link to the index and its body', async () => {
    await pace()
    const made = page('March 2025', '<p>body</p>')
    assert.ok(made.includes('<title>March 2025 · ledgerline</title>'))
    assert.ok(made.includes('<a href="index.html">ledgerline</a>'))
    assert.ok(made.includes('<h1>March 2025</h1>\n<p>body</p>'))
  })

  it('makes a table and a list of links', async () => {
    await pace()
    const made = table(['month', 'total'], [['March', '452.40 EUR']])
    assert.ok(made.includes('<th>month</th><th>total</th>'))
    assert.ok(made.includes('<tr><td>March</td><td>452.40 EUR</td></tr>'))
    assert.equal(links([['a.html', 'A & B']]), '<ul>\n<li><a href="a.html">A &amp; B</a></li>\n</ul>')
  })
})
