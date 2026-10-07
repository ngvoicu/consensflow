import { readFileSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:https'
import { join } from 'node:path'
import { archiveOf, plistValue, run } from './bundle.mjs'
import { signFile } from './signing.mjs'

/**
 * The feed the apps of the smoke ask, as a release's `latest.json` is, and the
 * archive it names, served over HTTPS on this machine's loopback with a
 * certificate made for the run. The app is told where the feed is by the
 * packaged self-test (`CONSENSFLOW_SELFTEST_UPDATER_URL`, loopback HTTPS only)
 * and which certificate holds its address; the feed's own archive address is
 * checked as a release's is (a GitHub asset of this version), and the archive is
 * fetched from this server.
 */

const openssl = (args) => run('openssl', args, { stdio: ['ignore', 'ignore', 'pipe'] })

/** A certificate authority and a server certificate for localhost, valid for a day. */
export function makeTls(directory) {
  const [caKey, caCert, serverKey, serverCsr, serverCert, extensions] = [
    'root.key',
    'root.pem',
    'server.key',
    'server.csr',
    'server.pem',
    'server.ext',
  ].map((name) => join(directory, name))
  writeFileSync(
    extensions,
    [
      'subjectAltName=DNS:localhost,IP:127.0.0.1',
      'basicConstraints=critical,CA:FALSE',
      'keyUsage=critical,digitalSignature,keyEncipherment',
      'extendedKeyUsage=serverAuth',
      'subjectKeyIdentifier=hash',
      'authorityKeyIdentifier=keyid,issuer',
      '',
    ].join('\n'),
  )
  const newKey = ['-newkey', 'rsa:2048', '-nodes']
  const root = ['-x509', '-days', '1', '-subj', '/CN=ConsensFlow updater smoke root']
  openssl(['req', ...root, ...newKey, '-keyout', caKey, '-out', caCert])
  openssl(['req', ...newKey, '-keyout', serverKey, '-out', serverCsr, '-subj', '/CN=localhost'])
  const signing = ['-CA', caCert, '-CAkey', caKey, '-CAcreateserial', '-sha256', '-days', '1']
  openssl([
    'x509',
    '-req',
    '-in',
    serverCsr,
    ...signing,
    '-extfile',
    extensions,
    '-out',
    serverCert,
  ])
  return { caCert, key: readFileSync(serverKey), cert: readFileSync(serverCert) }
}

/** The feed's file name for the archive of `version`: a release's, which the app checks. */
export const archiveName = (version) => `ConsensFlow-${version}_aarch64.app.tar.gz`

/** The release the feed offers. */
export function feedDocument(version, signature) {
  return {
    version,
    notes: 'Packaged updater acceptance candidate.',
    pub_date: '2026-09-09T12:00:00Z',
    platforms: {
      'darwin-aarch64': {
        url: `https://github.com/ngvoicu/consensflow/releases/download/v${version}/${archiveName(version)}`,
        signature,
      },
    },
  }
}

/**
 * The update of `app`, signed: its archive in `directory`, and the signature of
 * those bytes by `privateKey` (the signer of the checkout `tauri` is of).
 */
export function signedUpdate({ tauri, privateKey, app, directory }) {
  const version = plistValue(app, 'CFBundleShortVersionString')
  const file = archiveOf(app, join(directory, archiveName(version)))
  return { version, bytes: readFileSync(file), signature: signFile(tauri, privateKey, file) }
}

/**
 * Serves the feed at `/feed` and the archive at `/archive`: each what `offer`
 * last set, and not found before. Closes with the case.
 */
export async function serveUpdates(tls) {
  let feed = null
  let archive = null
  const server = createServer({ key: tls.key, cert: tls.cert }, (request, response) => {
    const body = { '/feed': feed, '/archive': archive }[request.url]
    if (body === undefined || body === null) {
      response.writeHead(404).end('not found')
      return
    }
    response.writeHead(200, {
      'content-type': request.url === '/feed' ? 'application/json' : 'application/gzip',
      'content-length': body.length,
    })
    response.end(body)
  })
  server.on('tlsClientError', (error) => process.stderr.write(`updater TLS: ${error.message}\n`))
  await new Promise((listening, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', listening)
  })
  const { port } = server.address()
  return {
    url: `https://127.0.0.1:${port}/feed`,
    /** Offers `version` with `signature`, whose archive is `bytes`: what the app downloads. */
    offer(version, signature, bytes) {
      feed = Buffer.from(JSON.stringify(feedDocument(version, signature)))
      archive = bytes
    },
    /** Offers nothing: the feed is not found. */
    withdraw() {
      feed = null
      archive = null
    },
    close: () => new Promise((closed) => server.close(closed)),
  }
}
