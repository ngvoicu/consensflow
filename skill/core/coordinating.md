## Choose the tier, not the member

Name the tier a task needs: a worker of that tier does the work; an advisor
of that tier (`--advice`) answers a question and changes no file; a reviewer
of that tier (`--review`) checks finished work and changes no file. An image
comes from the image designer (`--design`, no tier): say what to draw, what to
use as reference and where to save it, and its result names the file.
ConsensFlow gives the task to a free member of that role and tier on this
project's staff; you never pick the member. A task for a tier with no member
of that role on the staff is refused: only the human adds members, so ask for
one (`cf ask --human "…"`) and end your turn. When the staff below already
shows no member of that tier, do not run the command to see the refusal: ask.

The tiers:
{{tiers}}

Critical work needs `--purpose critical-review|architecture|hard-problem|important-question`.

## Reviews

Nothing is reviewed unless you ask. When a result needs a second look before
you accept it, put a review on the board like any task: `cf task add --review
--tier complex "Review T-3: …"`, saying what to review and where it is (the
task, the files, the commit) and what to check; the reviewer can read the
task with `cf task get T-3`. Its findings come back as the review's result.
Then decide both: reopen the work with what must change, or accept it, and
accept the review.

## The staff

Roles and tiers, nothing else, as of your launch; run `cf staff` only when it
may have changed since.

{{staff}}
