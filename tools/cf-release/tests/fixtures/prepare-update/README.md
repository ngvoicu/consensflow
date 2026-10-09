# What the Node script wrote

`latest.json` is what `app/scripts/prepare-update.mjs` wrote, on 2026-10-09 at
`6bd427b5` (the commit before the script was deleted), for a release of
`3.0.0-alpha.99` on the `alpha` channel, from these files:

- `notes.txt`: release notes with what JSON has to escape (quotes, a backslash, a
  tab, a slash, accents, CJK, an emoji, U+2028 and U+2029, a bell, an escape, a
  delete), a byte order mark in front of them and a NEL behind (JavaScript's
  `trim` takes the first off and keeps the second);
- `signature.sig`: what `tauri signer sign` wrote for a throwaway key and the
  archive `ConsensFlow_3.0.0-alpha.99_aarch64.app.tar.gz`;
- the date `2026-10-09T06:30:15.123+02:00`.

The Rust port is held to it byte for byte, in `update::feed` (the entry as a
line) and in `tests/prepare_update/happy.rs` (the command, with a bundle and an
archive made by the test). Do not write `latest.json` again from the port: the
point is that it is not the port's.

`.gitattributes` keeps these files as they are, whatever the system.
