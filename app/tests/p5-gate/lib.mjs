// The gate's browser side: the page's own modules (app/ui/term.js, app/ui/terminal-link.js, the
// shipped xterm) served as the app serves them, in a blank page with page.js's helpers (P5).
// From T-2096's lib.mjs; since P5-C the page under test is the real one, not a stand-in.
import { createRequire } from 'node:module'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
// The checkout this folder is in (app/tests/p5-gate/ -> the root); CF_TREE names another.
export const tree =
  process.env.CF_TREE ?? resolve(dirname(fileURLToPath(import.meta.url)), '../../..')
const require = createRequire(resolve(tree, 'app/package.json'))
const pw = require('@playwright/test')
const { serveUi } = await import(resolve(tree, 'app/tests/serve-ui.mjs'))
export const pageScript = resolve(dirname(fileURLToPath(import.meta.url)), 'page.js')

const BLANK =
  '<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="/vendor/xterm.css"><body></body>'

/** A browser with the blank page served from the checkout's app/ui, P5 loaded. */
export async function open(browserName) {
  const ui = await serveUi()
  const browser = await pw[browserName].launch({ headless: true })
  const page = await browser.newPage()
  page.on('pageerror', (e) => console.error('PAGE ERROR', e.message))
  await page.route('**/p5-blank', (route) =>
    route.fulfill({ contentType: 'text/html', body: BLANK }),
  )
  await page.goto(`${ui.origin}/p5-blank`)
  await page.addScriptTag({ path: pageScript })
  await page.evaluate(() => window.P5.load())
  return { browser, page, ui }
}
