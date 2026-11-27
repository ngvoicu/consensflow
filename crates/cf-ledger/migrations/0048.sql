-- The closed form of an account hold (T-2353): which of billing or login refused the member,
-- and the status and the provider's error type its refusal carried, as JSON
-- `{"kind":…,"status":…,"code":…}`, each value a literal of `cf_proto::hold`'s lists. It is the
-- hold's own state: written in the statement that makes the hold, NULL in every statement that
-- clears it or makes another hold, and read by the board only through `HoldForm::checked`.
-- A hold made before this has NULL, and the board says its generic sentence. Every row is kept.
ALTER TABLE participant ADD COLUMN out_form TEXT;
