// Makes the installers' verifier fixtures in this folder again, from their sources:
//
//   node tests/fixtures/install/make-fixtures.mjs
//
// - ed25519-rfc8032.json: RFC 8032's test vectors for Ed25519 (section 7.1), read
//   from the RFC's text at rfc-editor.org, each checked here by signing its
//   message with its secret key through Node's crypto before it is written;
// - blake2b-512.json: BLAKE2b-512 of RFC 7693's "abc" (Appendix A), and of
//   messages of the lengths around a block's 128 bytes, as Node's crypto
//   (OpenSSL) answers;
// - tauri-signed/: a SHA256SUMS as a release names it, signed by
//   `tauri signer sign` (app/node_modules) with a key made for it and thrown
//   away, and that key's public line;
// - release-signature/: the update archive's signature of a published release,
//   ConsensFlow 3.0.6-alpha.2, by ConsensFlow's own update key.

import { Buffer } from 'node:buffer'
import { spawnSync } from 'node:child_process'
import crypto from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(here, '../../..')

async function download(url) {
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url}: ${response.status}`)
  return Buffer.from(await response.arrayBuffer())
}

async function rfc8032() {
  const lines = (await download('https://www.rfc-editor.org/rfc/rfc8032.txt'))
    .toString('utf8')
    .split('\n')
  const start = lines.findIndex((line) => line.startsWith('7.1.  Test Vectors for Ed25519'))
  const end = lines.findIndex((line, at) => at > start && line.startsWith('7.2.  '))
  const vectors = []
  let current = null
  let field = null
  for (const raw of lines.slice(start, end)) {
    if (/^Josefsson|^RFC 8032|\f/.test(raw)) continue
    const line = raw.trim()
    const test = line.match(/^-----TEST (.+)$/)
    if (test) {
      current = { name: `TEST ${test[1]}`, secret: '', public: '', message: '', signature: '' }
      vectors.push(current)
      field = null
    } else if (current && line === 'SECRET KEY:') field = 'secret'
    else if (current && line === 'PUBLIC KEY:') field = 'public'
    else if (current && line.startsWith('MESSAGE')) field = 'message'
    else if (current && line === 'SIGNATURE:') field = 'signature'
    else if (current && field && /^[0-9a-f]+$/.test(line)) current[field] += line
  }
  for (const vector of vectors) {
    const der = Buffer.concat([
      Buffer.from('302e020100300506032b657004220420', 'hex'),
      Buffer.from(vector.secret, 'hex'),
    ])
    const key = crypto.createPrivateKey({ key: der, format: 'der', type: 'pkcs8' })
    const own = crypto
      .createPublicKey(key)
      .export({ format: 'der', type: 'spki' })
      .subarray(12)
      .toString('hex')
    const signed = crypto.sign(null, Buffer.from(vector.message, 'hex'), key).toString('hex')
    if (own !== vector.public || signed !== vector.signature)
      throw new Error(`${vector.name} was read wrong`)
  }
  if (vectors.length !== 5)
    throw new Error(`RFC 8032 7.1 has 5 vectors, ${vectors.length} were read`)
  return {
    source: 'RFC 8032, section 7.1 (https://www.rfc-editor.org/rfc/rfc8032.txt)',
    vectors: vectors.map(({ name, public: key, message, signature }) => ({
      name,
      public: key,
      message,
      signature,
    })),
  }
}

function blake2b() {
  const lengths = [
    0, 1, 2, 3, 63, 64, 111, 112, 127, 128, 129, 200, 255, 256, 257, 383, 384, 385, 1000, 1024,
  ]
  const vectors = lengths.map((length) => {
    const message = Buffer.alloc(length)
    for (let i = 0; i < length; i++) message[i] = (i * 7 + 3) & 255
    return {
      message: message.toString('hex'),
      hash: crypto.createHash('blake2b512').update(message).digest('hex'),
    }
  })
  const abc =
    'ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923'
  if (crypto.createHash('blake2b512').update('abc').digest('hex') !== abc)
    throw new Error("Node's BLAKE2b-512 disagrees with RFC 7693")
  return {
    source:
      'RFC 7693 Appendix A ("abc"); the rest as Node\'s crypto (OpenSSL) answers, for messages of byte i = (7i + 3) mod 256',
    vectors: [{ message: Buffer.from('abc').toString('hex'), hash: abc }, ...vectors],
  }
}

function tauriSigned() {
  const tauri = path.join(root, 'app/node_modules/.bin/tauri')
  const work = mkdtempSync(path.join(os.tmpdir(), 'cf-tauri-signed-'))
  try {
    const name = 'ConsensFlow_9.9.9-test.1_SHA256SUMS'
    const sums = [
      `${'1'.repeat(64)}  ConsensFlow_9.9.9-test.1_aarch64.dmg`,
      `${'2'.repeat(64)}  ConsensFlow_9.9.9-test.1_x64-setup.exe`,
      '',
    ].join('\n')
    writeFileSync(path.join(work, name), sums)
    const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '', CI: 'true' }
    for (const words of [
      ['signer', 'generate', '--ci', '-p', '', '-w', path.join(work, 'test.key')],
      ['signer', 'sign', '-f', path.join(work, 'test.key'), name],
    ]) {
      const ran = spawnSync(tauri, words, { cwd: work, env, encoding: 'utf8' })
      if (ran.status !== 0) throw new Error(`tauri ${words.join(' ')}: ${ran.stderr}`)
    }
    const version = spawnSync(tauri, ['--version'], { encoding: 'utf8' }).stdout.trim()
    const out = path.join(here, 'tauri-signed')
    mkdirSync(out, { recursive: true })
    writeFileSync(path.join(out, name), sums)
    writeFileSync(path.join(out, `${name}.sig`), readFileSync(path.join(work, `${name}.sig`)))
    const keyLine = Buffer.from(readFileSync(path.join(work, 'test.key.pub'), 'utf8'), 'base64')
      .toString('utf8')
      .split('\n')[1]
    writeFileSync(
      path.join(out, 'key.json'),
      `${JSON.stringify({ signer: version, file: name, keyLine }, null, 2)}\n`,
    )
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

async function releaseSignature() {
  const out = path.join(here, 'release-signature')
  mkdirSync(out, { recursive: true })
  const name = 'ConsensFlow_3.0.6-alpha.2_aarch64.app.tar.gz.sig'
  writeFileSync(
    path.join(out, name),
    await download(
      `https://github.com/ngvoicu/consensflow/releases/download/v3.0.6-alpha.2/${name}`,
    ),
  )
}

writeFileSync(
  path.join(here, 'ed25519-rfc8032.json'),
  `${JSON.stringify(await rfc8032(), null, 2)}\n`,
)
writeFileSync(path.join(here, 'blake2b-512.json'), `${JSON.stringify(blake2b(), null, 2)}\n`)
tauriSigned()
await releaseSignature()
