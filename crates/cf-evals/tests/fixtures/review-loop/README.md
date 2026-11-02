# The page the stand-in reviews

`assessment.html` is the page a Haiku 4.5 worker wrote for the `review-blockers` run of 2026-11-02
(T-1044, T-1 of that run, returned whole in its result): the English translation of
`evals/fixtures/site/site/evaluare.html`, a minimal one, as the chief's brief asked for it. The
stand-in's findings (`crates/cf-evals/src/scenarios/review_rounds.rs`) are said of a page like
this one, and `tests/scenarios.rs` holds each of them true of it: a note that a faithful minimal
translation does not deserve (the title that lacked the site name, the colours' order, the
sentence that was two already) was false against the real page in that run, and the chief said so.

The two pages of the scripted worker's later answers, made from these by one phrase each, and held
to it by `tests/scenarios.rs`:
- `assessment-fixed.html` is `assessment.html` with "a score between 0 and 100" replaced by "a score on a
  scale from 1 to 5" (the second answer, a reopen for F1);
- `evaluare-fixed.html` is the Romanian page of the fixture, `evals/fixtures/site/site/evaluare.html`,
  with "un scor între 0 și 100" replaced by "un scor pe o scară de la 1 la 5" (the third answer, which
  fixes both pages).
