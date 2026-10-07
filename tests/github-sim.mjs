import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { basename, dirname, join } from 'node:path'

/**
 * GitHub on this machine, as far as the release workflow touches it: the
 * releases and their assets that `gh` makes (the commands app/scripts/publish.mjs
 * runs, and no others: one it does not know, or a flag it does not know, is a
 * failure, so a command the workflow starts using is noticed here), and the
 * addresses installed apps and the checks download from, which serve the
 * published ones (a draft's assets are not served).
 *
 *   const github = await githubSim()
 *   github.base                 where files are downloaded from: <base>/<tag>/<name>
 *   github.gh(args, { cwd })    one `gh` call: { status, stdout, stderr }
 *   github.release(tag, ...)    a release as it already is
 *   github.fail(fn)             fn(args, index) says why a call fails, or nothing
 *   github.override(path, v)    what a download address serves instead: a status, a body, 'reset'
 *                               (the connection dropped), or a function giving one of them
 *   github.requests             the download addresses asked for
 *   github.trace                after each call: the call, and every release's assets
 *
 * `POST /__gh` runs a call for a `gh` that is a process of its own (the
 * workflow's step, run as written).
 */
export async function githubSim({ repo = 'ngvoicu/consensflow' } = {}) {
  const releases = new Map()
  const overrides = new Map()
  const requests = []
  const trace = []
  let failing = () => undefined
  let calls = 0
  let nextId = 1000

  const names = (tag) => [...releases.get(tag).assets.keys()].sort()
  const snapshot = () =>
    Object.fromEntries(
      [...releases].map(([tag, one]) => [tag, { draft: one.draft, assets: names(tag) }]),
    )
  const done = (stdout = '') => ({ status: 0, stdout, stderr: '' })
  const refuse = (stderr, status = 1) => ({ status, stdout: '', stderr: `${stderr}\n` })

  /** The words a command's arguments say, by what each flag takes. */
  function parse(rest, { values = [], booleans = [] }) {
    const positional = []
    const flags = {}
    for (let at = 0; at < rest.length; at += 1) {
      const arg = rest[at]
      if (!arg.startsWith('-')) {
        positional.push(arg)
        continue
      }
      const [name, joined] = arg.replace(/^--?/, '').split(/=(.*)/s)
      if (joined !== undefined) flags[name] = joined
      else if (values.includes(name)) {
        flags[name] = rest[at + 1]
        at += 1
      } else if (booleans.includes(name)) flags[name] = true
      else throw new Error(`the simulator does not know the flag ${arg}`)
    }
    return { positional, flags }
  }

  const found = (tag) => (releases.has(tag) ? releases.get(tag) : null)
  const assetJson = (tag, name, asset) => ({
    apiUrl: `https://api.github.com/repos/${repo}/releases/assets/${asset.id}`,
    contentType: 'application/octet-stream',
    id: `RA_${asset.id}`,
    label: '',
    name,
    size: asset.data.length,
    state: 'uploaded',
    url: `https://github.com/${repo}/releases/download/${tag}/${name}`,
  })

  function release(verb, rest, cwd) {
    if (verb === 'view') {
      const { positional, flags } = parse(rest, { values: ['json'] })
      const one = found(positional[0])
      if (one === null) return refuse('release not found')
      const document = {
        assets: [...one.assets].map(([name, asset]) => assetJson(positional[0], name, asset)),
        isDraft: one.draft,
        isPrerelease: one.prerelease,
        name: one.title,
        body: one.notes,
      }
      const wanted = String(flags.json).split(',')
      return done(
        `${JSON.stringify(Object.fromEntries(wanted.map((key) => [key, document[key]])))}\n`,
      )
    }
    if (verb === 'create') {
      const { positional, flags } = parse(rest, {
        values: ['title', 'notes', 'notes-file'],
        booleans: ['draft', 'prerelease', 'verify-tag'],
      })
      if (positional.length !== 1) throw new Error('the simulator makes a release with no files')
      const [tag] = positional
      if (releases.has(tag)) return refuse('HTTP 422: Validation Failed (already_exists)')
      releases.set(tag, {
        draft: flags.draft === true,
        prerelease: flags.prerelease === true,
        title: flags.title ?? tag,
        notes:
          flags.notes ??
          (flags['notes-file'] === undefined
            ? ''
            : readFileSync(join(cwd, flags['notes-file']), 'utf8')),
        assets: new Map(),
      })
      return done(`https://github.com/${repo}/releases/tag/${tag}\n`)
    }
    if (verb === 'upload') {
      const { positional } = parse(rest, {})
      const [tag, ...files] = positional
      const one = found(tag)
      if (one === null) return refuse('release not found')
      for (const file of files) {
        const name = basename(file)
        if (one.assets.has(name)) return refuse(`a file named ${name} already exists`)
        one.assets.set(name, { id: nextId++, data: readFileSync(join(cwd, file)) })
      }
      return done()
    }
    if (verb === 'delete-asset') {
      const { positional } = parse(rest, { booleans: ['yes', 'y'] })
      const [tag, name] = positional
      const one = found(tag)
      if (one === null) return refuse('release not found')
      if (!one.assets.delete(name)) return refuse(`asset under the name "${name}" not found`)
      return done()
    }
    if (verb === 'edit') {
      const { positional, flags } = parse(rest, { values: ['title', 'notes', 'draft'] })
      const one = found(positional[0])
      if (one === null) return refuse('release not found')
      if (flags.title !== undefined) one.title = flags.title
      if (flags.notes !== undefined) one.notes = flags.notes
      if (flags.draft !== undefined) one.draft = flags.draft !== 'false'
      return done()
    }
    throw new Error(`the simulator does not know gh release ${verb}`)
  }

  /** `gh api --method PATCH repos/<repo>/releases/assets/<id> -f name=<new>`: an asset renamed. */
  function api(rest) {
    const { positional, flags } = parse(rest, { values: ['method', 'f'] })
    const match = new RegExp(`^repos/${repo}/releases/assets/(\\d+)$`).exec(positional[0])
    if (flags.method !== 'PATCH' || match === null || !/^name=/.test(flags.f ?? '')) {
      throw new Error(`the simulator does not know gh api ${rest.join(' ')}`)
    }
    for (const one of releases.values()) {
      for (const [name, asset] of one.assets) {
        if (asset.id !== Number(match[1])) continue
        const renamed = flags.f.slice('name='.length)
        if (one.assets.has(renamed)) return refuse('HTTP 422: Validation Failed (already_exists)')
        one.assets.delete(name)
        one.assets.set(renamed, asset)
        return done('{}\n')
      }
    }
    return refuse('HTTP 404: Not Found')
  }

  function gh(args, { cwd = process.cwd() } = {}) {
    const index = calls
    calls += 1
    const why = failing(args, index)
    let answer
    if (why !== undefined && why !== null && why !== false) answer = refuse(String(why))
    else {
      try {
        const [group, ...rest] = args
        if (group === 'release') answer = release(rest[0], rest.slice(1), cwd)
        else if (group === 'api') answer = api(rest)
        else throw new Error(`the simulator does not know gh ${group}`)
      } catch (error) {
        answer = refuse(error.message, 2)
      }
    }
    trace.push({ args, status: answer.status, after: snapshot() })
    return answer
  }

  const server = createServer((request, response) => {
    if (request.method === 'POST' && request.url === '/__gh') {
      let body = ''
      request.on('data', (chunk) => {
        body += chunk
      })
      request.on('end', () => {
        const { args, cwd } = JSON.parse(body)
        response.writeHead(200).end(JSON.stringify(gh(args, { cwd })))
      })
      return
    }
    requests.push(request.url)
    const instead = overrides.get(request.url)
    const forced = typeof instead === 'function' ? instead() : instead
    if (forced === 'reset') {
      request.socket.destroy()
      return
    }
    const [, tag, name] = /^\/([^/]+)\/([^/]+)$/.exec(request.url) ?? []
    const one = found(tag)
    const asset = one === null || one.draft ? undefined : one.assets.get(name)
    const body = forced ?? asset?.data
    if (typeof body === 'number') response.writeHead(body).end()
    else if (body === undefined) response.writeHead(404).end()
    else if (request.headers.range === undefined) response.writeHead(200).end(body)
    else response.writeHead(206).end(Buffer.from(body).subarray(0, 1))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const base = `http://127.0.0.1:${server.address().port}`

  return {
    base,
    repo,
    gh,
    trace,
    /** The download addresses asked for, in order. */
    requests,
    get calls() {
      return trace.map(({ args }) => args)
    },
    /** A release as it already is: its assets (name to bytes), and whether it is a draft. */
    release(tag, { draft = false, prerelease = false, assets = {} } = {}) {
      releases.set(tag, {
        draft,
        prerelease,
        title: tag,
        notes: '',
        assets: new Map(
          Object.entries(assets).map(([name, data]) => [
            name,
            { id: nextId++, data: Buffer.from(data) },
          ]),
        ),
      })
    },
    /** An asset's bytes replaced where it is, as no `gh` call does: a file that is not the one uploaded. */
    put(tag, name, data) {
      releases.get(tag).assets.get(name).data = Buffer.from(data)
    },
    /** An asset gone where it is, as no `gh` call of the publisher does: a file somebody else deleted. */
    remove(tag, name) {
      releases.get(tag).assets.delete(name)
    },
    has: (tag) => releases.has(tag),
    isDraft: (tag) => releases.get(tag).draft,
    isPrerelease: (tag) => releases.get(tag).prerelease,
    title: (tag) => releases.get(tag).title,
    notes: (tag) => releases.get(tag).notes,
    names: (tag) => (releases.has(tag) ? names(tag) : null),
    asset: (tag, name) => releases.get(tag)?.assets.get(name)?.data ?? null,
    /** Every call from now on is asked of `fn(args, index)`: a reason it fails, or nothing. */
    fail(fn) {
      failing = fn
    },
    override(path, value) {
      if (value === undefined) overrides.delete(path)
      else overrides.set(path, value)
    },
    close() {
      // The client keeps its connections alive; the server would wait for them.
      server.closeAllConnections()
      return new Promise((resolve) => server.close(resolve))
    },
  }
}

/** A release's latest.json, naming its archive where the release publishes it under `base`. */
export function latestJson(base, version, notes = 'notes') {
  return `${JSON.stringify({
    version,
    notes,
    pub_date: '2026-10-06T12:00:00Z',
    platforms: {
      'darwin-aarch64': {
        url: `${base}/v${version}/ConsensFlow_${version}_aarch64.app.tar.gz`,
        signature: 'signed',
      },
    },
  })}\n`
}

/** What a built release's folder holds, where `files` maps a path in it to its bytes. */
export function builtFiles(base, version) {
  const mac = `ConsensFlow_${version}_aarch64`
  const text = (path) => `${path} of ${version}\n`
  return {
    'notes.txt': `ConsensFlow ${version}, the notes\n`,
    [`${mac}.dmg`]: text('dmg'),
    [`${mac}.app.tar.gz`]: text('archive'),
    [`${mac}.app.tar.gz.sig`]: text('signature'),
    'latest.json': latestJson(base, version),
    [`nsis/ConsensFlow_${version}_x64-setup.exe`]: text('installer'),
    [`portable/ConsensFlow_${version}_x64-portable.exe`]: text('portable'),
  }
}

/**
 * A built release's files as the published release holds them, by file name,
 * with the SHA256SUMS the workflow adds (`<sha256>  <name>`, as sha256sum writes
 * them): what `builtFiles` made, less the notes.
 */
export function publishedAssets(files) {
  const published = {}
  for (const [path, data] of Object.entries(files)) {
    if (path !== 'notes.txt') published[basename(path)] = data
  }
  const sums = Object.entries(published)
    .map(([name, data]) => `${createHash('sha256').update(data).digest('hex')}  ${name}\n`)
    .join('')
  return { ...published, SHA256SUMS: sums }
}

/**
 * The bytes of a ustar archive of the files `names` (paths in it, each holding
 * its own name), made as the release makes its own: what `tar -t` lists is what
 * the old apps' updater is held to.
 */
export function archiveOf(names) {
  const dir = mkdtempSync(join(tmpdir(), 'cf-archive-'))
  try {
    const tree = join(dir, 'tree')
    for (const name of names) {
      mkdirSync(dirname(join(tree, name)), { recursive: true })
      writeFileSync(join(tree, name), name)
    }
    const target = join(dir, 'archive.tar.gz')
    execFileSync('tar', ['--format', 'ustar', '-czf', target, '-C', tree, 'ConsensFlow.app'], {
      env: { ...process.env, COPYFILE_DISABLE: '1' },
    })
    return readFileSync(target)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/** A folder of `files` (a path in it mapped to its bytes), made as `npm test` makes its own: in the system's. */
export function folderOf(files) {
  const dir = mkdtempSync(join(tmpdir(), 'cf-release-'))
  for (const [path, data] of Object.entries(files)) {
    const target = join(dir, ...path.split('/'))
    mkdirSync(dirname(target), { recursive: true })
    writeFileSync(target, data)
  }
  return dir
}
