// What the installers' tests and their run against a real release share: a
// signer of the shape Tauri's (`tauri signer sign`) leaves, test copies of the
// scripts that trust a test key, and a local server for a scripted feed.
//
// A test copy differs from the script it copies in the lines it names alone,
// each found exactly once: the key the script trusts, and, for the Mac's tests
// of a fake app, the checks only a real notarized ConsensFlow passes.

import { Buffer } from 'node:buffer'
import crypto from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const INSTALL_SH = path.join(ROOT, 'install/install.sh')
export const INSTALL_PS1 = path.join(ROOT, 'install/install.ps1')

/** The minisign key line app/src-tauri/tauri.conf.json holds, inside its own base64. */
export function updaterKeyLine() {
  const config = JSON.parse(readFileSync(path.join(ROOT, 'app/src-tauri/tauri.conf.json'), 'utf8'))
  const text = Buffer.from(config.plugins.updater.pubkey, 'base64').toString('utf8')
  return text.split('\n')[1]
}

/** The key line a script trusts: `key=…` in install.sh, `$key = '…'` in install.ps1. */
export function scriptKeyLine(text) {
  const found = [...text.matchAll(/^ {4}(?:key=([A-Za-z0-9+/=]+)|\$key = '([A-Za-z0-9+/=]+)')$/gm)]
  if (found.length !== 1)
    throw new Error(`the script names its key ${found.length} times, not once`)
  return found[0][1] ?? found[0][2]
}

/** The name of the file whose signature the installers take for `version`'s SHA256SUMS. */
export function signedName(version) {
  return `ConsensFlow_${version}_SHA256SUMS`
}

/**
 * A new Ed25519 key, as minisign writes it, and a signer of files with it: a
 * .sig file's text as Tauri's signer leaves it (minisign's prehashed "ED"
 * signature over BLAKE2b-512, a trusted comment `timestamp:<n>\tfile:<name>`
 * and its global signature, the whole in base64).
 */
export function makeKey() {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519')
  const raw = publicKey.export({ format: 'der', type: 'spki' }).subarray(12)
  const id = crypto.randomBytes(8)
  const keyLine = Buffer.concat([Buffer.from('Ed'), id, raw]).toString('base64')
  const sign = (bytes, fileName, { timestamp = 1793000000, comment } = {}) => {
    const prehashed = crypto.createHash('blake2b512').update(bytes).digest()
    const signature = crypto.sign(null, prehashed, privateKey)
    const trusted = comment ?? `timestamp:${timestamp}\tfile:${fileName}`
    const global = crypto.sign(
      null,
      Buffer.concat([signature, Buffer.from(trusted, 'latin1')]),
      privateKey,
    )
    const text = [
      'untrusted comment: signature from tauri secret key',
      Buffer.concat([Buffer.from('ED'), id, signature]).toString('base64'),
      `trusted comment: ${trusted}`,
      global.toString('base64'),
      '',
    ].join('\n')
    return Buffer.from(text, 'latin1').toString('base64')
  }
  return { keyLine, sign }
}

/**
 * The check the installers make, made by Node's crypto (OpenSSL) instead:
 * whether `sigText` (a .sig file's text, as Tauri's signer leaves it) is a
 * minisign "ED" signature of `bytes` by the key `keyLine`, with a trusted
 * comment `timestamp:<n>\tfile:<fileName>` its global signature holds. Null
 * when it holds, else what does not.
 */
export function minisignProblem(keyLine, sigText, bytes, fileName) {
  // Base64 as the installers read it: nothing but its letters, padded, and no stray bits.
  const strict = (text) => {
    const decoded =
      /^[A-Za-z0-9+/]*={0,2}$/.test(text) && text.length % 4 === 0
        ? Buffer.from(text, 'base64')
        : null
    return decoded !== null && decoded.toString('base64') === text ? decoded : Buffer.alloc(0)
  }
  const key = strict(keyLine)
  if (key.length !== 42 || key.subarray(0, 2).toString('latin1') !== 'Ed')
    return 'the public key is not a minisign Ed25519 key'
  const lines = strict(sigText.trim()).toString('latin1').split('\n')
  if (
    lines.length !== 5 ||
    lines[4] !== '' ||
    !lines[0].startsWith('untrusted comment: ') ||
    !lines[2].startsWith('trusted comment: ')
  ) {
    return 'the signature file is not a minisign signature'
  }
  const signature = strict(lines[1])
  const global = strict(lines[3])
  if (signature.length !== 74 || global.length !== 64)
    return 'the signature file is not a minisign signature'
  if (signature.subarray(0, 2).toString('latin1') !== 'ED')
    return "the signature is not of minisign's prehashed kind (ED)"
  if (!signature.subarray(2, 10).equals(key.subarray(2, 10)))
    return 'the signature was made by another key'
  const publicKey = crypto.createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]),
    format: 'der',
    type: 'spki',
  })
  const prehashed = crypto.createHash('blake2b512').update(bytes).digest()
  if (!crypto.verify(null, prehashed, publicKey, signature.subarray(10)))
    return 'the signature does not match the file'
  const comment = lines[2].slice('trusted comment: '.length)
  if (
    !crypto.verify(
      null,
      Buffer.concat([signature.subarray(10), Buffer.from(comment, 'latin1')]),
      publicKey,
      global,
    )
  ) {
    return "the trusted comment's signature does not match"
  }
  const named = comment.match(/^timestamp:[0-9]+\tfile:(.*)$/)
  if (named === null || named[1] !== fileName)
    return `the signature is for ${named === null ? 'something else' : named[1]}, not ${fileName}`
  return null
}

/** `text` with each `[find, replace]` made, each `find` found exactly once. */
export function replaceOnce(text, replacements) {
  let out = text
  for (const [find, replace] of replacements) {
    const count = out.split(find).length - 1
    if (count !== 1)
      throw new Error(`the script holds ${JSON.stringify(find)} ${count} times, not once`)
    out = out.replace(find, () => replace)
  }
  return out
}

/** Writes to `dest` a copy of the script at `source` that trusts `keyLine`, with `replacements` made too. */
export function testCopy(source, dest, keyLine, replacements = []) {
  const text = readFileSync(source, 'utf8')
  const key = scriptKeyLine(text)
  writeFileSync(dest, replaceOnce(text, [[key, keyLine], ...replacements]))
  return dest
}

/** The lines of SHA256SUMS for `files`, `{ name: bytes }`, as cf-publish writes them. */
export function sumsOf(files) {
  return Object.entries(files)
    .map(([name, bytes]) => `${crypto.createHash('sha256').update(bytes).digest('hex')}  ${name}\n`)
    .join('')
}

/**
 * Serves `files`, `{ "/path": bytes }`, from 127.0.0.1 (anything else is a 404),
 * and records each path asked for. The map can be changed while it serves.
 */
export async function serve(files) {
  const asked = []
  const server = http.createServer((request, response) => {
    const at = decodeURIComponent(new URL(request.url, 'http://localhost').pathname)
    asked.push(at)
    const body = files[at]
    if (body === undefined) {
      response.writeHead(404).end('not found')
      return
    }
    response.writeHead(200, { 'content-length': Buffer.byteLength(body) }).end(body)
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address()
  return {
    url: `http://127.0.0.1:${port}`,
    asked,
    close: () => new Promise((resolve) => server.close(resolve)),
  }
}

/**
 * The files of a scripted release `version` under `releases` (a path such as
 * "/releases"): `artifacts` (`{ name: bytes }`), their SHA256SUMS, and its
 * signature by `key` unless `signature` says otherwise (null for none, or a
 * function of the SHA256SUMS text and the default signature giving another).
 */
export function scriptedRelease(releases, version, artifacts, key, { sums, signature } = {}) {
  const text = sums ?? sumsOf(artifacts)
  const made = key.sign(Buffer.from(text, 'latin1'), signedName(version))
  const sig =
    signature === undefined
      ? made
      : typeof signature === 'function'
        ? signature(text, made)
        : signature
  const out = {}
  for (const [name, bytes] of Object.entries(artifacts))
    out[`${releases}/download/v${version}/${name}`] = bytes
  out[`${releases}/download/v${version}/SHA256SUMS`] = text
  if (sig !== null) out[`${releases}/download/v${version}/SHA256SUMS.sig`] = sig
  return out
}

/** A feed's latest.json naming `version`, as the update feeds have it. */
export function feedOf(releases, channel, version) {
  return {
    [`${releases}/download/feed-${channel}/latest.json`]: JSON.stringify({
      version,
      notes: `ConsensFlow ${version}\n\nNotes that mention "version": "0.0.1" in passing.`,
      pub_date: '2026-10-27T00:00:00Z',
      platforms: {},
    }),
  }
}
