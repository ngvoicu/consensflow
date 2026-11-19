-- A Resume has no brief. Its new anchor identifies that handover without
-- deriving a holding from historical messages.
CREATE TABLE question_resume_anchor (
    id INTEGER PRIMARY KEY,
    participant_id INTEGER NOT NULL REFERENCES participant(id) ON DELETE CASCADE,
    task_id INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE
);
-- Holdings and bindings are recorded prospectively; existing messages are untouched.
CREATE TABLE question_holding (
    participant_id INTEGER PRIMARY KEY REFERENCES participant(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    task_id INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
    first_brief_id INTEGER NOT NULL,
    last_brief_id INTEGER NOT NULL,
    closed INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE question_binding (
    question_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    participant_id INTEGER NOT NULL REFERENCES participant(id) ON DELETE CASCADE,
    task_id INTEGER REFERENCES task(id) ON DELETE CASCADE,
    first_brief_id INTEGER,
    closed INTEGER NOT NULL DEFAULT 0
);
-- A handover can be refused after admission; keep its prior holding until settled.
CREATE TABLE question_handover (
    brief_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
    participant_id INTEGER NOT NULL REFERENCES participant(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL,
    previous_task_id INTEGER,
    previous_first_brief_id INTEGER,
    previous_last_brief_id INTEGER,
    previous_closed INTEGER,
    previous_run_id TEXT,
    admission_unknown INTEGER NOT NULL DEFAULT 0
);
