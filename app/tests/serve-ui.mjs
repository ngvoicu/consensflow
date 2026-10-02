import { readFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { dirname, extname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'

const UI_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', 'ui')
const TYPES = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
}

/**
 * The page's own files (app/ui, and nothing outside it) on a free local
 * port, for a spec to open under its stand-in of the app. Never cached: each
 * test reads the page as it is on disk.
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
      response.end(body)
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
