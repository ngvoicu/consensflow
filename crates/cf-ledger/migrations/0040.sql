-- A plan's retry key and its original names and task numbers commit with its tasks.
CREATE TABLE task_plan_key (
    project_id INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    tasks TEXT NOT NULL,
    PRIMARY KEY (project_id, key)
);
