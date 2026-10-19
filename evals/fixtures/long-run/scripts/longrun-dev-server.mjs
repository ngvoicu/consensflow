/**
 * `npm run dev`: serves dist/ (or the folder named after it) and a small API, and
 * never exits. It listens on 127.0.0.1, on `PORT` (4173 unless set), or the next
 * port that is free when that one is taken, and says where; then a line a minute,
 * to say it still serves.
 *
 *   GET /                         dist/index.html
 *   GET /<page>.html              that page of dist/
 *   GET /api/balances?month=M     each account's balance in month M, in cents
 */
import { readFile } from 'node:fs/promises'
import { createServer } from 'node:http'
import { extname, join } from 'node:path'

import { config } from '../src/config.mjs'
import { load } from '../src/index.mjs'
import { byMonth } from '../src/ledger.mjs'

const TYPES = { '.html': 'text/html; charset=utf-8', '.css': 'text/css', '.json': 'application/json' }
const root = process.argv[2] ?? config.dist
const first = Number(process.env.PORT ?? 4173)
const books = load(config.data)
const months = byMonth(books.entries, config.currency)

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? '/', 'http://127.0.0.1')
  if (url.pathname === '/api/balances') {
    const month = url.searchParams.get('month') ?? ''
    const balances = months.get(month)
    response.writeHead(balances ? 200 : 404, { 'content-type': TYPES['.json'] })
    response.end(JSON.stringify(balances ? Object.fromEntries(balances) : { error: `no month ${month}` }))
    return
  }
  const file = url.pathname === '/' ? 'index.html' : decodeURIComponent(url.pathname.slice(1))
  if (file.includes('..') || file.includes('\\')) {
    response.writeHead(403).end('no')
    return
  }
  try {
    const body = await readFile(join(root, file))
    response.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream' })
    response.end(body)
  } catch {
    response.writeHead(404, { 'content-type': 'text/plain' }).end(`no ${file} in ${root}/`)
  }
})

/** Listens on `port`, or the next one when it is taken (twenty tries). */
function listen(port) {
  server.once('error', (error) => {
    if (error.code === 'EADDRINUSE' && port < first + 20) listen(port + 1)
    else throw error
  })
  server.listen(port, '127.0.0.1', () => {
    console.log(`serving ${root}/ on http://127.0.0.1:${server.address().port}`)
  })
}

listen(first)
setInterval(() => console.log(`still serving (${new Date().toISOString()})`), 60_000)
