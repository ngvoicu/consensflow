import { readFileSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { dirname, extname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const APP = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const UI_ROOT = resolve(APP, 'ui')
const TYPES = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
}

/**
 * The app's Content-Security-Policy, as `tauri.conf.json` says it: Tauri sends
 * it with the page, adding nothing of its own for this page (it adds a nonce
 * only to a `<script src="http…">` or a `<style>`, and a hash only for an
 * inline script, none of which the page has).
 */
export const POLICY = JSON.parse(readFileSync(resolve(APP, 'src-tauri', 'tauri.conf.json'), 'utf8'))
  .app.security.csp

/**
 * What the page's policy refuses on `page` from now on, as Chromium tells it on
 * the console: a suite holds each test to none, so the policy is shown to
 * break nothing the page does.
 */
export function refusedByPolicy(page) {
  const refused = []
  page.on('console', (message) => {
    if (message.text().includes('Content Security Policy')) refused.push(message.text())
  })
  return refused
}

/** The page's HTML with the policy as the first thing its head says after its charset. */
function withPolicy(html) {
  if (!POLICY) return html
  const meta = `<meta http-equiv="Content-Security-Policy" content="${POLICY.replaceAll('"', '&quot;')}">`
  return html.replace(/(<meta charset="utf-8">)/i, `$1\n  ${meta}`)
}

/**
 * The page's own files (app/ui, and nothing outside it) on a free local
 * port, for a spec to open under its stand-in of the app, under the app's own
 * policy. Never cached: each test reads the page as it is on disk.
 */
export async function serveUi() {
  const server = createServer(async (request, response) => {
    try {
      const pathname = new URL(request.url, 'http://localhost').pathname
      const file = resolve(
        UI_ROOT,
        pathname === '/' ? 'index.html' : decodeURIComponent(pathname.slice(1)),
      )
      if (file !== UI_ROOT && !file.startsWith(`${UI_ROOT}${sep}`)) {
        response.writeHead(403).end('forbidden')
        return
      }
      // Read before the head is written, so a missing file can still answer 404.
      const body = await readFile(file)
      response.writeHead(200, {
        'content-type': TYPES[extname(file)] ?? 'application/octet-stream',
        'cache-control': 'no-store',
      })
      response.end(extname(file) === '.html' ? withPolicy(body.toString('utf8')) : body)
    } catch {
      response.writeHead(404).end('not found')
    }
  })
  await new Promise((listening) => server.listen(0, '127.0.0.1', listening))
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    close: () => new Promise((closed) => server.close(closed)),
  }
}
