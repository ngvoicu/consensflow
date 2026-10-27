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
//   ConsensFlow 3.0.6-alpha.2, by ConsensFlow's own update key;
// - ed25519-edges.json: encodings that are a point and ones that are not (y at
//   or above p, x = 0 with the sign bit set, a y whose x would be the root of a
//   non-square, by Euler's criterion), and signatures by a fixed key over
//   messages whose R || A || M ends at SHA-512's padding boundaries, each
//   checked through Node's crypto before it is written;
// - sha512.json: SHA-512 at every length 0 to 300 and some larger, as Node's
//   crypto answers;
// - ed25519-torsion.json: signatures with small-order and mixed-order points in
//   A and in R, and what RFC 8032 5.1.7's equation (encode([S]B - [k]A) = R,
//   no cofactor) answers for each, by a reference made here with BigInt;
// - powershell-parser-cases.json: Astraeus's crypto audit's cases (T-806): a
//   NUL after the signature's last line feed, and a signed file name with a NUL
//   or a soft hyphen after it, which a culture comparison takes as equal.
//
// `node make-fixtures.mjs edges` makes ed25519-edges.json and sha512.json alone
// (the others are made again only when no word is given: tauri-signed/ gets a
// new key each time).

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

const P = 2n ** 255n - 19n

function power(base, exponent) {
  let result = 1n
  let b = base % P
  let e = exponent
  while (e > 0n) {
    if (e & 1n) result = (result * b) % P
    b = (b * b) % P
    e >>= 1n
  }
  return result
}

function encoding(y, sign) {
  const out = Buffer.alloc(32)
  let v = y
  for (let i = 0; i < 32; i++) {
    out[i] = Number(v & 255n)
    v >>= 8n
  }
  if (sign) out[31] |= 0x80
  return out.toString('hex')
}

function edges() {
  const d = (((-121665n * power(121666n, P - 2n)) % P) + P) % P
  // x^2 = (y^2 - 1) / (d y^2 + 1): a point where that is a square (or 0), none where it is not.
  const square = (y) => {
    const x2 = (((((y * y - 1n) % P) + P) % P) * power((d * y * y + 1n) % P, P - 2n)) % P
    return x2 === 0n || power(x2, (P - 1n) / 2n) === 1n
  }
  const offCurve = []
  for (let y = 2n; offCurve.length < 2; y++) if (!square(y)) offCurve.push(y)
  const points = [
    { name: 'the identity (y = 1)', point: encoding(1n, false), decodes: true },
    { name: 'y = 0, a point of order 4', point: encoding(0n, false), decodes: true },
    { name: 'y = p', point: encoding(P, false), decodes: false },
    { name: 'y = p + 1, the identity not reduced', point: encoding(P + 1n, false), decodes: false },
    { name: 'y = 2^255 - 1', point: encoding(2n ** 255n - 1n, false), decodes: false },
    { name: 'x = 0 with the sign bit (y = 1)', point: encoding(1n, true), decodes: false },
    { name: 'x = 0 with the sign bit (y = p - 1)', point: encoding(P - 1n, true), decodes: false },
    ...offCurve.map((y) => ({
      name: `y = ${y}, off the curve`,
      point: encoding(y, false),
      decodes: false,
    })),
  ]
  // A fixed key, so the signatures are the same each time they are made.
  const seed = crypto
    .createHash('sha256')
    .update('consensflow installers: padding boundaries')
    .digest()
  const key = crypto.createPrivateKey({
    key: Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]),
    format: 'der',
    type: 'pkcs8',
  })
  const publicKey = crypto.createPublicKey(key)
  const pub = publicKey.export({ format: 'der', type: 'spki' }).subarray(12).toString('hex')
  // SHA-512 of R || A || M (64 bytes and the message) ends a block at 111, 112, 127, 128,
  // 239 and 240 bytes in all.
  const boundaries = [47, 48, 63, 64, 175, 176].map((length) => {
    const message = Buffer.alloc(length)
    for (let i = 0; i < length; i++) message[i] = (i * 13 + 5) & 255
    const signature = crypto.sign(null, message, key)
    if (!crypto.verify(null, message, publicKey, signature))
      throw new Error(`${length}: not signed`)
    return {
      name: `R || A || M of ${64 + length} bytes`,
      public: pub,
      message: message.toString('hex'),
      signature: signature.toString('hex'),
    }
  })
  return {
    source:
      "make-fixtures.mjs: points by rule and by Euler's criterion; signatures by Node's crypto",
    points,
    boundaries,
  }
}

function sha512() {
  const lengths = [...Array(301).keys(), 511, 512, 513, 1000, 1024]
  return {
    source: "Node's crypto (OpenSSL), for messages of byte i = (11i + 7) mod 256",
    vectors: lengths.map((length) => {
      const message = Buffer.alloc(length)
      for (let i = 0; i < length; i++) message[i] = (i * 11 + 7) & 255
      return {
        message: message.toString('hex'),
        hash: crypto.createHash('sha512').update(message).digest('hex'),
      }
    }),
  }
}

const only = process.argv.slice(2)
if (only.length === 0) {
  writeFileSync(
    path.join(here, 'ed25519-rfc8032.json'),
    `${JSON.stringify(await rfc8032(), null, 2)}\n`,
  )
  writeFileSync(path.join(here, 'blake2b-512.json'), `${JSON.stringify(blake2b(), null, 2)}\n`)
  tauriSigned()
  await releaseSignature()
}
// A reference of Ed25519's group for the torsion cases, as RFC 8032 5.1 writes it (extended
// coordinates), made here apart from the scripts' own.
const L = 2n ** 252n + 27742317777372353535851937790883648493n
const D = (((-121665n * power(121666n, P - 2n)) % P) + P) % P
const mod = (a) => ((a % P) + P) % P
const I = power(2n, (P - 1n) / 4n)
function addPoints([x1, y1, z1, t1], [x2, y2, z2, t2]) {
  const a = mod((y1 - x1) * (y2 - x2))
  const b = mod((y1 + x1) * (y2 + x2))
  const c = mod(2n * D * t1 * t2)
  const d = mod(2n * z1 * z2)
  const [e, f, g, h] = [b - a, d - c, d + c, b + a]
  return [mod(e * f), mod(g * h), mod(f * g), mod(e * h)]
}
function times(n, point) {
  let result = [0n, 1n, 1n, 0n]
  let p = point
  for (let k = n; k > 0n; k >>= 1n) {
    if (k & 1n) result = addPoints(result, p)
    p = addPoints(p, p)
  }
  return result
}
function encodePoint([x, y, z]) {
  const zi = power(z, P - 2n)
  const ax = mod(x * zi)
  return encoding(mod(y * zi), ax & 1n)
}
function pointOf(y, odd) {
  const x2 = mod((y * y - 1n) * power(mod(D * y * y + 1n), P - 2n))
  let x = power(x2, (P + 3n) / 8n)
  if (mod(x * x - x2) !== 0n) x = mod(x * I)
  if ((x & 1n) !== (odd ? 1n : 0n)) x = mod(P - x)
  return [x, y, 1n, mod(x * y)]
}
const littleEndian = (hex) => {
  const bytes = Buffer.from(hex, 'hex')
  let v = 0n
  for (let i = bytes.length - 1; i >= 0; i--) v = (v << 8n) | BigInt(bytes[i])
  return v
}
const scalarHex = (n) => encoding(n, false)
const challenge = (r, a, m) =>
  littleEndian(
    crypto
      .createHash('sha512')
      .update(Buffer.concat([Buffer.from(r, 'hex'), Buffer.from(a, 'hex'), m]))
      .digest()
      .toString('hex'),
  ) % L
// RFC 8032 5.1.7's check, with no cofactor, as the scripts make it.
function verifies(a, r, sHex, m) {
  const s = littleEndian(sHex)
  if (s >= L) return false
  const k = challenge(r, a, m)
  const A = pointOf(littleEndian(a) & ((1n << 255n) - 1n), littleEndian(a) >> 255n === 1n)
  // -[k]A as [k]A negated: [L - k]A would be it only for a point of order L.
  const kA = times(k, A)
  const negKA = [mod(-kA[0]), kA[1], kA[2], mod(-kA[3])]
  return encodePoint(addPoints(times(s, B), negKA)) === r
}
const B = pointOf(mod(4n * power(5n, P - 2n)), false)

function torsion() {
  const identity = encoding(1n, false)
  const T = pointOf(0n, false) // a point of order 4: (sqrt(-1), 0)
  const t = encodePoint(T)
  // A fixed key a, a fixed nonce r: P = aB, R = rB.
  const a =
    littleEndian(
      crypto
        .createHash('sha512')
        .update('consensflow installers: torsion key')
        .digest()
        .subarray(0, 32)
        .toString('hex'),
    ) % L
  const r =
    littleEndian(
      crypto
        .createHash('sha512')
        .update('consensflow installers: torsion nonce')
        .digest()
        .subarray(0, 32)
        .toString('hex'),
    ) % L
  const honest = times(a, B)
  const mixed = encodePoint(addPoints(honest, T))
  const R = encodePoint(times(r, B))
  /** A message, `M <i>`, the first whose challenge k with `a` and `r` is (or is not) a multiple of 4. */
  const message = (rHex, aHex, multiple) => {
    for (let i = 0; ; i++) {
      const m = Buffer.from(`M ${i}`)
      if ((challenge(rHex, aHex, m) % 4n === 0n) === multiple) return m
    }
  }
  const cases = []
  const add = (name, aHex, rHex, s, m) =>
    cases.push({
      name,
      public: aHex,
      message: m.toString('hex'),
      signature: rHex + scalarHex(s),
      expect: verifies(aHex, rHex, scalarHex(s), m),
    })
  add('A and R the identity, S = 0', identity, identity, 0n, Buffer.from('any message'))
  add('A and R the identity, S = 1', identity, identity, 1n, Buffer.from('any message'))
  const m4 = message(identity, t, true)
  add('A of order 4, R the identity, S = 0, k a multiple of 4', t, identity, 0n, m4)
  add(
    'A of order 4, R the identity, S = 0, k no multiple of 4',
    t,
    identity,
    0n,
    message(identity, t, false),
  )
  // A of mixed order (aB + T), signed as for aB: S = r + k a, with k over A = aB + T.
  for (const multiple of [true, false]) {
    const m = message(R, mixed, multiple)
    const k = challenge(R, mixed, m)
    add(
      `A of mixed order, k ${multiple ? 'a multiple of 4' : 'no multiple of 4'}`,
      mixed,
      R,
      (r + k * a) % L,
      m,
    )
  }
  // R of mixed order (rB + T) with an honest A: the equation without the cofactor refuses it.
  const mixedR = encodePoint(addPoints(times(r, B), T))
  const honestA = encodePoint(honest)
  const m = Buffer.from('mixed R')
  add('R of mixed order, A honest', honestA, mixedR, (r + challenge(mixedR, honestA, m) * a) % L, m)
  // And the honest signature itself, which holds.
  add('R and A honest', honestA, R, (r + challenge(R, honestA, m) * a) % L, m)
  return {
    source:
      "make-fixtures.mjs: RFC 8032 5.1.7's equation without the cofactor, by a BigInt reference",
    cases,
  }
}

if (only.length === 0 || only.includes('edges')) {
  writeFileSync(path.join(here, 'ed25519-torsion.json'), `${JSON.stringify(torsion(), null, 2)}\n`)
  writeFileSync(path.join(here, 'ed25519-edges.json'), `${JSON.stringify(edges(), null, 2)}\n`)
  writeFileSync(path.join(here, 'sha512.json'), `${JSON.stringify(sha512(), null, 2)}\n`)
}
