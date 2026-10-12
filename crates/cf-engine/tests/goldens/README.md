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

## Changed by hand after Node

The chief's text, in each of the twelve `roleInstructions` rows that hold it, has
three passages Node's did not: the line of its commands for `cf task add --test`, the
tester in the sentence that names what each role of a tier does, and the line of its
Reviews that says a tester checks work by running it. They were changed by hand, in
those words alone, when the tester was added (`skill/core/chief.md`), and no row of the
tester's own text was added: `../text/roles.rs` holds that.

The same twelve rows say "a crash" in the sentence about a window lost, where Node's
said "a restart or a crash" (`skill/core/chief.md`): a restart pauses no task any
more, for the project it suspends comes back by the human's Resume, which opens the
windows that had work. Changed by hand, in those words alone.
