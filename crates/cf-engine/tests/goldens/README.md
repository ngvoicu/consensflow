# What Node's engine said: the texts

`text.json` holds the texts of the engine as Node's modules answered them: how a
message reads in its recipient's pane (delivery text), the handoff a new chief
gets, and the role instructions each role is given. It was recorded from Node,
which is gone, and it is fixed: nothing records it again, and a change that
moves a text is made in the file by hand, on purpose, with the reason in the
commit. The recorder ran Node's modules and went with them; it is in the flip
release `v3.0.0-alpha.82`, and in `21297242`, the commit before it was deleted.

`../text/` plays it: `goldens_delivery.rs`, `goldens_handoff.rs` and
`goldens_roles.rs` each take their tables.
