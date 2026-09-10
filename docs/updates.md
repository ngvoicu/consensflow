# Release update metadata

`app/scripts/prepare-update.mjs` creates the static Tauri updater feed locally. It does not upload assets, invoke remote commands, modify an installed app, or read a signing key.

## Prepare a feed

Build the app and archive with the same version, then run the helper from the repository root. The current source version is `3.0.0-alpha.37`; replace paths and the publication date for the release being prepared.

```sh
node app/scripts/prepare-update.mjs \
  --repo "$PWD" \
  --bundle /path/to/ConsensFlow.app \
  --archive /path/to/ConsensFlow-3.0.0-alpha.37_aarch64.app.tar.gz \
  --signature /path/to/ConsensFlow-3.0.0-alpha.37_aarch64.app.tar.gz.sig \
  --notes /path/to/release-notes.txt \
  --output /path/to/latest.json \
  --channel alpha \
  --date 2026-09-09T12:00:00Z
```

The helper checks source, bundle, bundled CLI, and archive versions; compares archive bytes and modes against the supplied app; rejects traversal, links, and special archive entries; validates the Tauri outer-base64 minisign envelope; and writes deterministic JSON. Release notes are treated as text and are preserved, including URLs. The platform URL is constructed from the fixed official GitHub release path and the versioned archive filename.

Python 3 is required for the helper's read-only `tarfile` inspection. The archive is never extracted to disk.

## Signing

The official Tauri signer owns signature creation. The private key is outside both app state roots at `~/.tauri/consensflow-updater.key`; keep it out of the repository and never put keys under `~/.consensflow` or `~/.config/consensflow`. The metadata helper consumes only the resulting `.sig` text and never reads the private key.


## Publication order

After local validation and release approval, publish the immutable versioned archive, its `.sig`, and the matching DMG first. Only then publish the generated `latest.json` to the correct rolling GitHub tag: `update-alpha` or `update-stable`. The app consumes `latest.json` from those rolling feeds; this document intentionally leaves remote upload commands to the release operator.

Updates validate the signed ConsensFlow bundle and its own version. They do not inspect or constrain installed harness versions.

## Checking from the app

Use **ConsensFlow → Check for Updates…** in the macOS menu. The app also checks
quietly after startup and periodically, and offers a review notice when a newer
release is available. Download and installation require explicit actions.

Each channel requires a published `latest.json` at its rolling tag. A missing or
unavailable feed is a failed check, not evidence that the installed app is up to
date. A DMG-only GitHub release cannot satisfy the updater: publish the signed
archive and matching feed in the order above. Changing channels does not repair
a missing feed; each channel is checked independently.

On 2026-09-10, both rolling feeds returned HTTP 404 and the published releases
contained DMGs only. Local builds/reinstalls do not publish updater assets.
