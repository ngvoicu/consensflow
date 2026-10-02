# Releases and update feeds

A release is one tag. Pushing `v<version>` runs
`.github/workflows/release.yml`, which builds, checks and publishes everything
people download:

- **macOS (Apple silicon):** the DMG, and the signed update bundle
  (`ConsensFlow_<version>_aarch64.app.tar.gz` and its `.sig`) that installed
  apps fetch through the update feed.
- **Windows x64:** the installer and the portable zip, from
  `.github/workflows/windows-build.yml`. The Windows app does not update
  itself yet: people download the new installer or zip.
- `latest.json`, and `SHA256SUMS` over every file.

## Cutting a release

1. Set the new version in `package.json`, `app/src-tauri/Cargo.toml` and
   `app/src-tauri/tauri.conf.json` (they must agree), and commit.
2. Tag that commit with the release notes as the tag's message, and push it:

   ```sh
   git tag -a v3.0.0-alpha.63 -m "What changed, for the people updating."
   git push upstream v3.0.0-alpha.63
   ```

   A tag without a message gets the commit subjects since the previous release.

The workflow refuses a tag that does not name the source version. The Mac job
builds the app with no key in reach, runs the packaged smoke on the built
bundle, then in a step of its own packs the update bundle (the app as
installed apps unpack it: files and folders under `ConsensFlow.app`, plain
ustar) and signs it with the key, and writes `latest.json` with
`app/scripts/prepare-update.mjs`. Once the Mac and
Windows builds pass, it creates the GitHub release (a prerelease for an
`-alpha` version) with every file, and only then replaces `latest.json` on the
rolling feed installed apps read: `update-alpha` for every version, and
`update-stable` as well for a stable one. Last, it checks that the feed serves
the new file and that the archive it names downloads byte for byte. Releases
run one at a time.

**Run it by hand** (Actions → Release → Run workflow) on `main` to build,
sign and smoke-test the same files without publishing anything; the run
carries them as its downloads. Only `main` and version tags may read the
signing key, so a hand run from another branch stops at its first job.

## Signing

The update signing key lives at `~/.tauri/consensflow-updater.key` on the
maintainer's Mac, and a copy is the `TAURI_SIGNING_PRIVATE_KEY` secret of the
repository's `release` environment. That environment admits only `v*` tags and
`main`; only the release workflow's Mac job uses it, and within it only the
step that signs the update bundle, after the build; no third-party action runs
in that job. The key has no password. Keep it out of the repository and
out of `~/.consensflow` and `~/.config/consensflow`. A leaked key means a new
one: its public key goes into `tauri.conf.json`, and installed apps trust it
only after one manual DMG install.

## The update metadata helper

`app/scripts/prepare-update.mjs` writes `latest.json`. It checks the source,
bundle, bundled CLI and archive versions; compares the archive's bytes and
modes against the app; rejects traversal, links and special entries; validates
the Tauri minisign envelope; and writes deterministic JSON. It never reads the
private key, uploads anything or touches an installed app. Python 3 is needed
for its read-only `tarfile` check; the archive is never extracted to disk. The
release workflow runs it; by hand:

```sh
node app/scripts/prepare-update.mjs \
  --repo "$PWD" \
  --bundle /path/to/ConsensFlow.app \
  --archive /path/to/ConsensFlow_<version>_aarch64.app.tar.gz \
  --signature /path/to/ConsensFlow_<version>_aarch64.app.tar.gz.sig \
  --notes /path/to/release-notes.txt \
  --output /path/to/latest.json \
  --channel alpha \
  --date 2026-10-01T12:00:00Z
```

## Checking from the app

Use **ConsensFlow → Check for Updates…** in the macOS menu. The app also checks
quietly after startup and periodically, and offers a review notice when a newer
release is available. Download and installation require explicit actions.

Each channel requires a published `latest.json` at its rolling tag. A missing or
unavailable feed is a failed check, not evidence that the installed app is up to
date. A DMG-only GitHub release cannot satisfy the updater: the signed archive
and its feed are what it reads. Changing channels does not repair a missing
feed; each channel is checked independently.

## History

The first signed updater release, [3.0.0-alpha.43](https://github.com/ngvoicu/consensflow/releases/tag/v3.0.0-alpha.43),
was published on 2026-09-10 with its [Alpha feed](https://github.com/ngvoicu/consensflow/releases/download/update-alpha/latest.json).
It was built and published by hand; releases since are cut by the release
workflow. The Stable feed is created by the first stable release.

Publication verification downloaded all versioned assets and compared their
hashes, verified the archive with the app's pinned public key, and exercised the
native updater against the public feed: alpha.42 found/downloaded alpha.43 and
alpha.43 reported up to date.
