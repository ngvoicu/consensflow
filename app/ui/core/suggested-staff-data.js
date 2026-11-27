/**
 * The suggested staff, as the page has it until the daemon sends one (T-2404): a stand-in of the
 * data a release ships, in the shape the staff screens read (`suggested-staff.js`).
 *
 * `about` says in plain words what the staff is for. Each of `members` is one member:
 * - `harness`: the harness it runs on, by kind (`claude-code`, `codex`, `pi`, `devin`, `muse`);
 * - `model`: the model as its harness's list names it (Pi's `provider/id`), what Add member sends;
 * - `effort`: one of the efforts that list gives the model, or null where it gives none;
 * - `tier`: the tier of all its slots (none for the image designer);
 * - `roles`: the roles it holds at that tier, by id, in any order: one slot each;
 * - `designer: true` for the image designer, on Codex, with no model, effort or tier.
 *
 * This project's own staff on 2026-11-27 (`cf staff`).
 */
export const STAND_IN = {
  about:
    'The staff ConsensFlow itself is built with, and that we know works: Claude Code does the work and makes the hardest calls, and Codex reviews and tests it, a second family of models checking the first.',
  members: [
    {
      harness: 'claude-code',
      model: 'claude-opus-5-5',
      effort: 'medium',
      tier: 'critical',
      roles: ['worker'],
    },
    {
      harness: 'claude-code',
      model: 'claude-sonnet-5-5',
      effort: 'medium',
      tier: 'complex',
      roles: ['worker'],
    },
    {
      harness: 'claude-code',
      model: 'claude-opus-5-5',
      effort: 'xhigh',
      tier: 'standard',
      roles: ['frontend'],
    },
    {
      harness: 'claude-code',
      model: 'claude-fable-5-2',
      effort: 'high',
      tier: 'critical',
      roles: ['architect', 'advisor', 'reviewer'],
    },
    {
      harness: 'codex',
      model: 'gpt-6-2-astra',
      effort: 'xhigh',
      tier: 'complex',
      roles: ['reviewer'],
    },
    {
      harness: 'codex',
      model: 'gpt-6-2-astra',
      effort: 'xhigh',
      tier: 'standard',
      roles: ['researcher', 'reviewer', 'tester'],
    },
    { harness: 'codex', designer: true, roles: ['designer'] },
  ],
}
