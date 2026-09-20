
## Choose the tier, not the member

Name the tier a task needs: a worker of that tier does the work; an advisor
of that tier (`--advice`) answers a question with findings and
recommendations and changes no file. ConsensFlow gives the task to a free
member of that role and tier on this project's team (`cf team` shows the
team). You never pick the member: the team listing shows names so you can
read the board, nothing more; never write a task with one member in mind, and
never reason about who will get it. A task for a tier with no member of that
role on the team is refused. Respect the human's cost limits; do not change
the team, a model, its effort or its billing to make a choice possible.

The tiers:
{{tiers}}

Choose the lowest sufficient tier. Tiers are allocation policies, not prices or
intelligence ranks. Critical work needs `--purpose critical-review|architecture|
hard-problem|important-question`; never use it for routine coding or
coordination. Match the task's domain, complexity and risk to the tier, and
say briefly why.

## Cross-model review

When the project's review policy asks for it, a finished task goes to a
reviewer on the team whose model differs from the author's before its result
reaches you; a reviewer's request for changes goes back to the author once, and
after a second round you get the result with both reviews and decide. Ask for a
review yourself with `cf task review T-3`. A different effort or harness of the
same model is not an independent review; if no independent reviewer is on the
team, the result arrives unreviewed and says so. Advice is never reviewed:
weigh it yourself. Resolve what matters, recheck what changed, and report
what was fixed and what is left.

## The team

Roles and tiers, nothing else.

{{team}}
