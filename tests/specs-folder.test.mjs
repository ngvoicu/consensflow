import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  arrange,
  filesOf,
  firstFile,
  frontmatter,
  kindOf,
  linkTarget,
  measure,
  readRegistry,
  specFile,
} from '../app/ui/core/specs-folder.js'

/** What the specs viewer makes of a specs folder as the daemon reads it (app/ui/core/specs-folder.js). */

const file = (path, size = 1) => ({ name: path.split('/').at(-1), path, size })
const folder = (path, entries) => ({ name: path.split('/').at(-1), path, entries })
/** A tree as the daemon gives it: each folder's entries by name. */
const TREE = [
  folder('api-refactor', [file('api-refactor/SPEC.md')]),
  folder('assets', [file('assets/spec-runtime.js'), file('assets/spec-styles.css')]),
  file('notes.txt'),
  file('registry.md'),
  folder('unlisted', [file('unlisted/SPEC.html')]),
  folder('user-auth', [
    file('user-auth/SPEC.md'),
    folder('user-auth/artifacts', [file('user-auth/artifacts/notes.md')]),
    file('user-auth/interview-01.md'),
    file('user-auth/page-audit.md'),
    file('user-auth/research-01.md'),
    file('user-auth/research-10.md'),
    file('user-auth/research-9.md'),
  ]),
]
const ROWS = [
  { id: 'user-auth', title: 'User Auth System' },
  { id: 'api-refactor', title: 'API Refactoring' },
  { id: 'gone', title: 'A Spec With No Folder' },
]
const labels = (entries) =>
  entries.map((entry) => (entry.entries ? `${entry.label}/` : entry.label))

describe('the tree', () => {
  it('lists the registry, the specs in its order by title, the rest by name, and assets last, folded', () => {
    const arranged = arrange(TREE, ROWS)
    assert.deepEqual(labels(arranged), [
      'registry.md',
      'User Auth System/',
      'API Refactoring/',
      'unlisted/',
      'notes.txt',
      'assets/',
    ])
    assert.deepEqual(
      arranged.filter((entry) => entry.entries).map((entry) => [entry.name, entry.open]),
      [
        ['user-auth', true],
        ['api-refactor', true],
        ['unlisted', true],
        ['assets', false],
      ],
    )
  })

  it('lists inside a spec its spec, its research and interviews by number, then the rest by name, folders folded', () => {
    const auth = arrange(TREE, ROWS).find((entry) => entry.name === 'user-auth')
    assert.deepEqual(labels(auth.entries), [
      'SPEC.md',
      'research-01.md',
      'research-9.md',
      'research-10.md',
      'interview-01.md',
      'artifacts/',
      'page-audit.md',
    ])
    assert.equal(auth.entries.find((entry) => entry.name === 'artifacts').open, false)
  })

  it('names a spec by its id where the registry has no title for it, and orders by name with no registry', () => {
    assert.deepEqual(labels(arrange(TREE)).slice(0, 4), [
      'registry.md',
      'api-refactor/',
      'unlisted/',
      'user-auth/',
    ])
    assert.deepEqual(labels(arrange(TREE, [{ id: 'unlisted', title: '' }])).slice(1, 2), [
      'unlisted/',
    ])
  })

  it('leaves dotfiles out at every level, whoever answered', () => {
    const arranged = arrange([
      file('.DS_Store'),
      folder('a', [file('a/.DS_Store'), file('a/SPEC.md'), folder('a/.git', [file('a/.git/x')])]),
    ])
    assert.deepEqual(filesOf(arranged), ['a/SPEC.md'])
  })

  it('opens on the registry, else on the first spec; a spec opens on its own file', () => {
    const arranged = arrange(TREE, ROWS)
    assert.equal(firstFile(arranged), 'registry.md')
    const unregistered = arrange(TREE.filter((entry) => entry.name !== 'registry.md'))
    assert.equal(firstFile(unregistered), 'api-refactor/SPEC.md')
    assert.equal(specFile(arranged, 'unlisted'), 'unlisted/SPEC.html')
    assert.equal(specFile(arranged, 'gone'), null)
    assert.equal(firstFile(arrange([folder('assets', [file('assets/a.css')])])), 'assets/a.css')
    assert.equal(firstFile([]), null)
  })
})

describe('the registry', () => {
  it("reads Specmint's table: each row, its progress as done of total", () => {
    const registry = readRegistry(
      [
        '# Spec Registry',
        '',
        '| ID | Title | Status | Priority | Progress | Updated |',
        '|----|-------|--------|----------|----------|---------|',
        '| user-auth | User Auth \\| Login | active | high | 5/12 | 2026-10-14 |',
        '| api | API | paused | medium | — | 2026-10-09 |',
        '',
        'Notes after the table.',
      ].join('\n'),
    )
    assert.equal(registry.heading, 'Spec Registry')
    assert.deepEqual(
      registry.rows.map(({ id, title, status, done, total, progress }) => [
        id,
        title,
        status,
        done,
        total,
        progress,
      ]),
      [
        ['user-auth', 'User Auth | Login', 'active', 5, 12, '5/12'],
        ['api', 'API', 'paused', null, null, '—'],
      ],
    )
  })

  it('is none where the text holds no such table', () => {
    assert.equal(readRegistry('# Our specs\n\n| Spec | State |\n|---|---|\n| a | b |\n'), null)
    // As many columns as Specmint's, but other ones.
    assert.equal(
      readRegistry(
        '| A | B | C | D | E | F |\n|---|---|---|---|---|---|\n| 1 | 2 | 3 | 4 | 5 | 6 |',
      ),
      null,
    )
    assert.equal(
      readRegistry('| ID | Title | Status | Priority | Progress | Updated |\n| no rule |'),
      null,
    )
    assert.equal(readRegistry(''), null)
  })

  it('reads a line built to make a pattern read it again and again in a moment', () => {
    const head = '| ID | Title | Status | Priority | Progress | Updated |'
    const started = performance.now()
    for (const after of [
      `${' '.repeat(131_072)}X`,
      `|${' -'.repeat(65_536)} X`,
      `|${' :---: |'.repeat(16_384)} X`,
    ]) {
      assert.equal(readRegistry(`${head}\n${after}`), null)
    }
    // A cell of escaped pipes, each once a copy of the cell so far: 3 seconds, as it was.
    const pipes = '\\|'.repeat(250_000)
    const registry = readRegistry(
      `${head}\n|---|---|---|---|---|---|\n| a | ${pipes} | active | high | 1/2 | 2026 |`,
    )
    assert.equal(registry.rows[0].title, '|'.repeat(250_000))
    assert.equal(registry.rows[0].status, 'active')
    // The pattern it had took 9 seconds for the first of these.
    assert.ok(performance.now() - started < 500, `${performance.now() - started} ms`)
  })
})

describe('a link between files', () => {
  const files = new Set([
    'user-auth/SPEC.md',
    'user-auth/research-01.md',
    'api/SPEC.md',
    'registry.md',
  ])
  const from = 'user-auth/SPEC.md'

  it("leads to a file of the folder, from the file's own folder or from the folder's top", () => {
    assert.equal(linkTarget(from, 'research-01.md', '.specs', files), 'user-auth/research-01.md')
    assert.equal(
      linkTarget(from, './research-01.md#findings', '.specs', files),
      'user-auth/research-01.md',
    )
    assert.equal(linkTarget(from, '../api/SPEC.md', '.specs', files), 'api/SPEC.md')
    assert.equal(linkTarget(from, '../registry.md?x=1', '.specs', files), 'registry.md')
    assert.equal(linkTarget(from, 'research%2D01.md', '.specs', files), 'user-auth/research-01.md')
    // As an agent writes a path from the project's folder.
    assert.equal(linkTarget(from, '.specs/api/SPEC.md', '.specs', files), 'api/SPEC.md')
    assert.equal(linkTarget(from, 'docs/plans/api/SPEC.md', 'docs/plans', files), 'api/SPEC.md')
  })

  it('leads nowhere out of the folder, to anything with a scheme, an absolute path or the file itself', () => {
    for (const href of [
      '../../package.json',
      // Above the top and down again onto a file the folder has: still out.
      '../../registry.md',
      '../../.specs/registry.md',
      '../../../etc/passwd',
      '/etc/passwd',
      '\\\\server\\share',
      '//evil.example/x',
      'https://example.com/a.md',
      'javascript:alert(1)',
      'JaVaScRiPt:alert(1)',
      'data:text/html,<script>',
      'file:///etc/passwd',
      '#findings',
      '',
      'missing.md',
      '%E0%A4%A',
    ]) {
      assert.equal(linkTarget(from, href, '.specs', files), null, href)
    }
  })
})

describe('the frontmatter of a spec', () => {
  it('is its fields in order and the text after it', () => {
    const read = frontmatter('---\nid: a\ntitle: A: the first\ntags: [x, y]\n---\n\n# A\n')
    assert.deepEqual(read.fields, [
      ['id', 'a'],
      ['title', 'A: the first'],
      ['tags', '[x, y]'],
    ])
    assert.equal(read.body, '\n# A\n')
  })

  it('is none where the text does not start with one, or never closes it', () => {
    assert.equal(frontmatter('# A\n---\n'), null)
    assert.equal(frontmatter('---\nid: a\n'), null)
  })
})

describe('what a file is shown as', () => {
  it('goes by its extension, the registry at the top being its own', () => {
    assert.equal(kindOf('registry.md'), 'registry')
    assert.equal(kindOf('a/registry.md'), 'markdown')
    assert.equal(kindOf('a/SPEC.md'), 'markdown')
    assert.equal(kindOf('a/SPEC.HTML'), 'html')
    assert.equal(kindOf('a/mockup.htm'), 'html')
    assert.equal(kindOf('a/flow.PNG'), 'image')
    assert.equal(kindOf('a/logo.svg'), 'image')
    assert.equal(kindOf('a/SPEC.md.sha256'), 'text')
    assert.equal(kindOf('a/Makefile'), 'text')
  })

  it('says its size as the daemon does', () => {
    assert.deepEqual([0, 1023, 1024, 12_800, 3_400_000].map(measure), [
      '0 bytes',
      '1023 bytes',
      '1.0 KB',
      '12.5 KB',
      '3.2 MB',
    ])
  })
})
