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

The same twelve rows say, after the heredoc example, "Write a long brief to a file with
your file tool and pass it with `--file brief.md` instead: a shell can cut a long heredoc"
(`skill/core/chief.md`): a chief's heredoc that a shell cut sent the board a brief cut at
about 8.1k characters (the field report of 2026-10-15, B6). Changed by hand, in those
words alone, on 2026-10-16.

The members' text (worker, advisor, reviewer and image designer), in each of the 27
`roleInstructions` rows that hold it, has one sentence Node's did not, the last but one of
its paragraph about a window that is for one task: "Your answer ends your turn's work:
anything you left running is stopped then, so wait for what your result needs"
(`skill/core/staff.md`). It was changed by hand, in those words alone, on 2026-10-13, when
a worker's answer became its task's result at once and what its turn left running began
to be ended with it: an agent was seen leaving a check running and answering, which that
rule now ends.

The same 27 rows have one paragraph Node's did not, after the one about a removal's paths:
"Others work in this folder too, and not all of their work is committed. Never throw away
a change you did not make: no `git checkout` or `git restore` that overwrites a file
(`git restore --staged` of your own change is fine), and no `git reset --hard`,
`git stash` or `git clean` over work that is not committed; take a change of yours out by
hand. Say a file is restored only once you have checked it: its diff, or what it holds."
(`skill/core/staff.md`). It was changed by hand, in those words alone, on
2026-10-15, when a worker on the owner's Windows machine overwrote a file that held
uncommitted work with `git checkout`, and other workers said they had restored files they
had not.
