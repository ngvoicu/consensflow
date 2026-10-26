import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  askedUntilFree,
  droppedText,
  pagePoint,
  untakeable,
  withControl,
} from '../app/ui/core/file-drop.js'

/**
 * What a file dropped on a terminal window types (app/ui/core/file-drop.js):
 * each path as one word of the system's shell, which names no window takes,
 * what a Codex window on Windows would not get as it is, and
 * where the app says the drop is, in the page's pixels. The page's Playwright
 * suite holds the rest (app/tests/core-page.spec.mjs, "file drop").
 */

const MAC = false
const WINDOWS = true

/** Paths a Mac may drop and the page types: every mark a shell reads, and what no shell does. */
const MAC_PATHS = [
  '/Users/me/My Docs/a b.png',
  '/Users/me/Desktop/Screenshot 2026-10-15 at 10.30.00\u202fAM (2).png',
  "/Users/me/it's here/$HOME & [x]*.txt",
  '/Users/me/~tilde/#hash/!bang/^caret/`tick`/{a,b}/|pipe;semi<lt>gt?"dq"\\bs',
  '/a-b_c.d,e:f=g%h+i@j/k',
  '/Users/me/日本語/résumé—final 🙂.png',
]

/** The shells a Mac's terminal runs, those this machine has. */
const SHELLS =
  process.platform === 'win32'
    ? []
    : ['bash', 'zsh'].filter((shell) => spawnSync(shell, ['-c', 'true']).status === 0)

describe('file drop: the words a Mac types', () => {
  it('puts a backslash before a space, and writes a path without one as it is', () => {
    assert.equal(droppedText(['/Users/me/My Docs/a b.png'], MAC), '/Users/me/My\\ Docs/a\\ b.png')
    assert.equal(droppedText(['/Users/me/Docs/a.png'], MAC), '/Users/me/Docs/a.png')
  })

  it('puts a backslash before each character a shell reads as more than itself', () => {
    const special = ' !"#$&\'()*;<>?[\\]^`{|}~'
    assert.equal(
      droppedText([`/x${special}y`], MAC),
      `/x${[...special].map((c) => `\\${c}`).join('')}y`,
    )
  })

  it('writes a Mac screenshot name, its parentheses and its narrow no-break space', () => {
    assert.equal(
      droppedText(['/Users/me/Desktop/Screenshot 2026-10-15 at 10.30.00\u202fAM (2).png'], MAC),
      '/Users/me/Desktop/Screenshot\\ 2026-10-15\\ at\\ 10.30.00\u202fAM\\ \\(2\\).png',
    )
  })

  it('leaves every other character as it is: letters, digits, marks of a path, and non-ASCII', () => {
    for (const path of [
      '/a-b_c.d,e:f=g%h+i@j/k',
      '/Users/me/Documents/résumé—final.txt',
      '/Users/me/日本語/ファイル.txt',
      '/Users/me/🙂.png',
    ]) {
      assert.equal(droppedText([path], MAC), path)
    }
  })

  it('separates several paths with a space, and ends with none', () => {
    assert.equal(
      droppedText(['/a b/c.png', "/it's/d.txt", '/plain/e'], MAC),
      "/a\\ b/c.png /it\\'s/d.txt /plain/e",
    )
  })

  it('types nothing for no paths', () => {
    assert.equal(droppedText([], MAC), '')
  })

  for (const shell of SHELLS) {
    it(`is read back by ${shell} as the paths dropped, each one word`, () => {
      const read = spawnSync(shell, ['-c', `printf '%s\\0' ${droppedText(MAC_PATHS, MAC)}`], {
        encoding: 'utf8',
      })
      assert.equal(read.status, 0, read.stderr)
      assert.deepEqual(read.stdout.split('\0').slice(0, -1), MAC_PATHS)
    })
  }
})

describe('file drop: a name no window takes', () => {
  it('is one with a control character, a tab and a delete too, on any system', () => {
    const controls = [...Array(32).keys()].map((code) => String.fromCharCode(code)).concat('\x7f')
    for (const control of controls) {
      const path = `/Users/me/a${control}b.png`
      assert.equal(withControl(['/plain.png', path]), path, JSON.stringify(control))
      assert.equal(withControl([`C:\\a${control}b.png`]), `C:\\a${control}b.png`)
    }
  })

  it('is none of a path of printable characters, whatever else it holds', () => {
    assert.equal(
      withControl([...MAC_PATHS, 'C:\\a — b “c”.png', '/a\u0085b\u2028c.png']),
      undefined,
    )
  })
})

describe('file drop: the words Windows types', () => {
  it('puts a path with a space in double quotes, and writes one without as it is', () => {
    assert.equal(
      droppedText(['C:\\Users\\me\\My Pictures\\a b.png'], WINDOWS),
      '"C:\\Users\\me\\My Pictures\\a b.png"',
    )
    assert.equal(
      droppedText(['C:\\Users\\me\\Pictures\\a.png'], WINDOWS),
      'C:\\Users\\me\\Pictures\\a.png',
    )
  })

  it('quotes a share, a drive and a name with other marks only for a space', () => {
    assert.equal(
      droppedText(['\\\\server\\share\\x y.txt'], WINDOWS),
      '"\\\\server\\share\\x y.txt"',
    )
    assert.equal(droppedText(['D:\\'], WINDOWS), 'D:\\')
    assert.equal(droppedText(['C:\\a&b(1)^c;d$e%f!.txt'], WINDOWS), 'C:\\a&b(1)^c;d$e%f!.txt')
    assert.equal(droppedText(['C:\\Screenshot (2).png'], WINDOWS), '"C:\\Screenshot (2).png"')
  })

  it('writes non-ASCII as it is, a narrow no-break space too', () => {
    assert.equal(droppedText(['C:\\a\u202fb\\日本語.txt'], WINDOWS), 'C:\\a\u202fb\\日本語.txt')
  })

  it('separates several paths with a space, quoting each by itself, and ends with none', () => {
    assert.equal(
      droppedText(['C:\\a b\\c.png', 'C:\\plain.txt', 'C:\\d\\e f.txt'], WINDOWS),
      '"C:\\a b\\c.png" C:\\plain.txt "C:\\d\\e f.txt"',
    )
  })

  it('types nothing for no paths', () => {
    assert.equal(droppedText([], WINDOWS), '')
  })
})

describe('file drop: what a window cannot take as it is typed into it', () => {
  it('is nothing for a Devin window on Windows, marks, letters, joining marks and emoji alike', () => {
    for (const path of [
      'C:\\Users\\me\\My Pictures\\a b.png',
      'C:\\a — b “c” — d.png',
      'C:\\€5 → £3.txt',
      'C:\\10.30.00\u202fAM.png',
      'C:\\re\u0301sume\u0301.txt',
      'C:\\\u1100\u1161.txt',
      'C:\\🙂.png',
    ]) {
      assert.deepEqual(untakeable(path, 'devin', WINDOWS), [], path)
    }
  })

  it('is, for a Codex window on Windows, each character outside the Basic Multilingual Plane, once', () => {
    assert.deepEqual(untakeable('C:\\a — b “c” → €.png', 'codex', WINDOWS), [])
    assert.deepEqual(untakeable('C:\\🙂 and 🙂\\𝐀.png', 'codex', WINDOWS), ['🙂', '𝐀'])
  })

  it('is nothing elsewhere, Codex included', () => {
    assert.deepEqual(untakeable('/Users/me/🙂 — €.png', 'codex', false), [])
  })
})

describe('file drop: what the app lets the page do', () => {
  it('lets it do nothing more of the window for a drop: the app brings it in front, once, itself', () => {
    const capability = JSON.parse(
      readFileSync(
        fileURLToPath(new URL('../app/src-tauri/capabilities/default.json', import.meta.url)),
        'utf8',
      ),
    )
    // The drop's events need none of their own: `core:default` holds the `listen` of every event.
    assert.deepEqual(capability.permissions, [
      'core:default',
      'core:window:allow-maximize',
      'core:window:allow-unmaximize',
      'core:window:allow-set-size',
      'core:window:allow-set-position',
      'dialog:default',
    ])
  })
})

describe('file drop: where the app says the drop is', () => {
  it('is in the page’s pixels already on a Mac, whatever the display’s scale', () => {
    for (const ratio of [1, 2, 3]) {
      assert.deepEqual(pagePoint({ x: 640, y: 360 }, { windows: false, ratio }), { x: 640, y: 360 })
    }
  })

  it('is in device pixels on Windows, which the display’s scale divides into the page’s', () => {
    assert.deepEqual(pagePoint({ x: 1280, y: 720 }, { windows: true, ratio: 2 }), {
      x: 640,
      y: 360,
    })
    assert.deepEqual(pagePoint({ x: 750, y: 300 }, { windows: true, ratio: 1.5 }), {
      x: 500,
      y: 200,
    })
    assert.deepEqual(pagePoint({ x: 640, y: 360 }, { windows: true, ratio: 1 }), { x: 640, y: 360 })
  })
})

describe('file drop: a lookup the app is busy for', () => {
  const BUSY = { ok: false, busy: true, error: 'another lookup of short names is running' }
  const ANSWERED = { ok: true, paths: [{ short: 'C:\\A-BC~1.PNG' }] }

  /** An app that answers `answers` in turn, and the waits between them; it refuses to be asked forever. */
  function app(answers) {
    const asked = { times: 0, waits: [] }
    const ask = async () => {
      asked.times += 1
      if (asked.times > 1000) throw new Error('asked a thousand times: it never stops')
      return answers[Math.min(asked.times, answers.length) - 1]
    }
    const wait = async (ms) => {
      asked.waits.push(ms)
    }
    return { asked, ask, wait }
  }

  it('is asked again while another lookup runs, and its answer is the one once it is free', async () => {
    const { asked, ask, wait } = app([BUSY, BUSY, ANSWERED])
    assert.deepEqual(await askedUntilFree(ask, { every: 250, within: 60_000 }, wait), ANSWERED)
    assert.deepEqual(asked, { times: 3, waits: [250, 250] })
  })

  it('is asked once where the app is free, or refuses for another reason', async () => {
    for (const answer of [
      ANSWERED,
      { ok: false, error: 'it is not one of the last 16 drops the app heard' },
    ]) {
      const { asked, ask, wait } = app([answer])
      assert.deepEqual(await askedUntilFree(ask, { every: 250, within: 60_000 }, wait), answer)
      assert.deepEqual(asked, { times: 1, waits: [] })
    }
  })

  it('is given up after a while, the app’s refusal its answer', async () => {
    const { asked, ask, wait } = app([BUSY])
    assert.deepEqual(await askedUntilFree(ask, { every: 250, within: 1000 }, wait), BUSY)
    assert.deepEqual(asked, { times: 5, waits: [250, 250, 250, 250] })
  })
})
