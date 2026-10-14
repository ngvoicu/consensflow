-- What a project says of specs on its own, which its chief plans big work by
-- (Specmint's phased plans, kept in a folder of the project's, one variant of
-- the four): whether they are on, where they are kept, and the variant. A NULL
-- is not said: `specs` then follows what the human set for every project (off
-- unless they turned it on), `specs_folder` is `.specs` and `specs_variant`
-- is `core`. Three columns added to the project, which no row needs a value
-- in, and the checks of the two that name something from a short list.
  ALTER TABLE project ADD COLUMN specs INTEGER
    CONSTRAINT project_specs_check CHECK (specs IS NULL OR specs IN (0, 1));
  ALTER TABLE project ADD COLUMN specs_folder TEXT;
  ALTER TABLE project ADD COLUMN specs_variant TEXT
    CONSTRAINT project_specs_variant_check
    CHECK (specs_variant IS NULL OR specs_variant IN ('core', 'core-html', 'tdd', 'tdd-html'));
