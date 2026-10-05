/**
 * What `npm run parity:launch` holds of a root (`tests/parity/tree.mjs`): the
 * tree a plan changed, a CLI's own folders counted and never listed, and the
 * ways its root reads in the text a plan makes.
 */
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { changes, gained, rootForms, snapshot } from './parity/tree.mjs'

const WINDOWS = process.platform === 'win32'

/** A root with what the test puts in it, and its removal. */
function root(files = {}) {
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'cf-tree-'))
  const add = (name, text) => {
    fs.mkdirSync(path.dirname(path.join(folder, name)), { recursive: true })
    fs.writeFileSync(path.join(folder, name), text)
  }
  for (const [name, text] of Object.entries(files)) add(name, text)
  return { folder, add, cleanup: () => fs.rmSync(folder, { recursive: true, force: true }) }
}

test('a plan changed what it made, what it changed and what it removed, and no other path', () => {
  const home = root({ 'a/kept.txt': 'kept', 'a/changed.txt': 'was', 'gone.txt': 'x' })
  try {
    const before = snapshot(home.folder, [])
    home.add('a/changed.txt', 'is')
    home.add('a/made.txt', 'new')
    fs.rmSync(path.join(home.folder, 'gone.txt'))
    const made = changes(before.entries, snapshot(home.folder, []).entries)
    assert.deepEqual(
      made.map(({ path: name, kind, text }) => [name, kind, text]),
      [
        ['a/changed.txt', 'file', 'is'],
        ['a/made.txt', 'file', 'new'],
        ['gone.txt', 'removed', undefined],
      ],
    )
  } finally {
    home.cleanup()
  }
})

test('a folder a CLI owns is counted and never listed, nor what is in it', () => {
  const home = root({ 'cli/state/db': 'x', 'mine/file': 'y' })
  try {
    const before = snapshot(home.folder, ['cli'])
    home.add('cli/state/more', 'z')
    home.add('cli/other', 'w')
    home.add('mine/made', 'v')
    const after = snapshot(home.folder, ['cli'])
    assert.deepEqual([...after.entries.keys()].sort(), ['mine', 'mine/file', 'mine/made'])
    assert.deepEqual(gained(before.counts, after.counts), { cli: 2 })
    assert.deepEqual(gained(after.counts, after.counts), {})
    assert.deepEqual(
      changes(before.entries, after.entries).map((change) => change.path),
      ['mine/made'],
    )
  } finally {
    home.cleanup()
  }
})

test('a file is its text whole, a byte order mark too, or the hash of bytes that are no text', () => {
  const bytes = Buffer.from([0xff, 0xfe, 0x00])
  const home = root({ bom: '﻿text', binary: bytes })
  try {
    const { entries } = snapshot(home.folder, [])
    assert.equal(entries.get('bom').text, '﻿text')
    assert.equal(entries.get('binary').text, undefined)
    assert.equal(entries.get('binary').bytes, createHash('sha256').update(bytes).digest('hex'))
  } finally {
    home.cleanup()
  }
})

test('a mode is what the system gave the file or the folder, and none on Windows', () => {
  const home = root({ 'sealed/file': 'x' })
  try {
    fs.chmodSync(path.join(home.folder, 'sealed/file'), 0o600)
    fs.chmodSync(path.join(home.folder, 'sealed'), 0o700)
    const { entries } = snapshot(home.folder, [])
    assert.equal(entries.get('sealed').mode, WINDOWS ? null : 0o700)
    assert.equal(entries.get('sealed/file').mode, WINDOWS ? null : 0o600)
  } finally {
    home.cleanup()
  }
})

test('a link is its target, never followed', { skip: WINDOWS }, () => {
  const home = root()
  try {
    fs.symlinkSync('elsewhere', path.join(home.folder, 'link'))
    assert.deepEqual(snapshot(home.folder, []).entries.get('link'), {
      kind: 'link',
      target: 'elsewhere',
    })
  } finally {
    home.cleanup()
  }
})

test('a root reads as a file URL, in JSON, in a query and as a window names a program', () => {
  const folder = path.join(path.parse(os.tmpdir()).root, 'a b', "100%#'s")
  const forms = rootForms(folder)
  assert.equal(new URL(forms.fileUrl).pathname.includes('%20'), true)
  assert.match(forms.fileUrl, /%25%23/)
  assert.ok(forms.plain.includes(folder))
  assert.ok(forms.plain.includes(encodeURIComponent(folder)))
  assert.ok(forms.plain.includes(encodeURIComponent(folder).replaceAll("'", '%27')))
  assert.ok(forms.plain.includes(new URLSearchParams({ d: folder }).toString().slice(2)))
  assert.ok(forms.plain.includes(JSON.stringify(folder).slice(1, -1)))
  assert.ok(forms.plain.includes(folder.replaceAll('\\', '/')))
  assert.equal(new Set(forms.plain).size, forms.plain.length, 'each form once')
})

test('a root on a Windows drive reads without its drive too, last, as HOMEPATH holds it', () => {
  const folder = 'C:\\Users\\rhea\\AppData\\Local\\Temp\\consensflow launch %#-x\\node'
  assert.equal(rootForms(folder).plain.at(-1), folder.slice('C:'.length))
  const posix = '/tmp/consensflow launch %#-x/node'
  assert.equal(
    rootForms(posix).plain.includes(posix.slice(2)),
    false,
    'a root on no drive loses nothing',
  )
})
