//! Codex's memory job put a task on the board as the chief.
//!
//! Seen on 2026-10-21 on ConsensFlow 3.0.5, in the `sites` project, with a Codex chief
//! (T-1313; the issue `issues/codex-memories-post-to-the-board-as-the-chief.md` in
//! consensflow-sme, found by T-1312's probe): a task "from chief" appeared on the board
//! asking a worker to consolidate `~/.codex/memories`, the title of Codex's own consolidation
//! prompt. No command of the chief's had opened it. Codex's memory job is a thread inside the
//! window's own `codex app-server`, started after a turn, in the memories folder of the Codex
//! home (`$CODEX_HOME/memories`); it is given the config of the window's thread, which holds
//! ConsensFlow's role as its developer instructions and `CONSENSFLOW_*`, the token among them,
//! as the variables of its commands, and ConsensFlow's windows have no sandbox. So the job ran
//! `cf` as the chief, and its role told it every change goes on the board.
//!
//! Codex has no key that fences the job while memories stay on (Gabriel's decision, 2026-10-21).
//! The class fix, a token the board takes only from the window's own thread, is 3.0.8's native
//! control. What alpha.4 has is a guard: `cf` run with its working folder in the memories folder
//! changes nothing on the board.
//!
//! The stand-in Codex runs the job as its server does (`CF_TEST_CODEX_BACKEND=memory-job`): after
//! the chief's first turn, a thread of its own, with the config of the window's thread, runs the
//! `cf task add` the case names, in `$CODEX_HOME/memories`.
//!
//! What the owner should have seen: the job's task is refused, in words that say why; the board
//! holds the tasks of the chief's own commands alone; and the window's own `cf task add`, from
//! where it works, goes on being taken.

use std::path::Path;

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig, OPEN};
use serde_json::{json, Value};

use crate::{config_with_codex, secs, Outcome};

/// What the job asks of the board: the title of Codex's consolidation prompt, as T-9 of `sites`
/// had it.
const JOB_BRIEF: &str = "Consolidate ~/.codex/memories according to the user-provided Memory \
    Writing Agent: Phase 2 (Consolidation) specification";

/// The line of the stand-in's log that says what the job ran and what `cf` answered it.
fn memory_job(rig: &Rig) -> Option<Value> {
    let log = rig.var("CF_TEST_CODEX_LOG").unwrap_or_default();
    files::read_string(Path::new(log))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|line| line["run"] == "memory-job")
}

#[test]
fn a_task_codexs_memory_job_adds_from_the_memories_folder_is_refused_and_the_chief_still_adds_its_own(
) -> Outcome {
    let rig = Rig::start(
        config_with_codex()
            .var("CF_TEST_CODEX_BACKEND", "memory-job")
            .var(
                "CF_TEST_CODEX_MEMORY_JOB",
                format!("task add --tier standard :: {JOB_BRIEF}"),
            ),
    )?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [
            { "id": "codex-chief", "kind": "codex", "model": "fake" },
            { "id": "worker", "kind": "claude-code", "model": "fake", "workTier": "standard" },
        ],
    });
    rig.stand_ins(&roster)?;
    let project = Project::open_with_staff(&rig, "codex-chief", None, &[("worker", "worker")])?;
    let tier = project.tiers()?.get("worker").cloned().unwrap_or_default();
    assert_eq!(tier, "standard", "the job asks for the worker's tier");
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;

    // The chief's own task: the worker's result is the chief's first turn, and the memory job
    // runs once that turn is over.
    let own = rig.cf_in_window(
        &chief,
        ["task", "add", "--tier", &tier, "Reply with exactly: OWN_OK"],
    )?;
    assert_eq!(own.code, Some(0), "{own}");
    rig.wait_for("Codex's memory job to have run", secs(120), || {
        Ok(memory_job(&rig).is_some())
    })?;
    let job = memory_job(&rig).unwrap_or_default();

    // It ran where Codex runs it, with the window's variables, as the chief would.
    let memories = Path::new(rig.var("CODEX_HOME").unwrap_or_default()).join("memories");
    assert_eq!(
        Path::new(job["cwd"].as_str().unwrap_or_default()),
        memories,
        "the job ran in the memories folder: {job}"
    );
    assert!(
        job["variables"]
            .as_array()
            .is_some_and(|names| names.iter().any(|name| name == "CONSENSFLOW_TOKEN")),
        "the job had the window's token: {job}"
    );

    // The board refused it, and says why.
    assert_eq!(job["code"], 1, "the job's task was taken: {job}");
    let said = job["output"].as_str().unwrap_or_default();
    assert!(
        said.contains("Codex's memory job isn't on the board"),
        "the refusal does not say why: {said}"
    );
    assert!(
        project.task(2)?.is_null(),
        "the job's task is on the board: {}",
        project.task(2)?
    );

    // The window works from where it always has, and the next task is T-2.
    let again = rig.cf_in_window(
        &chief,
        [
            "task",
            "add",
            "--tier",
            &tier,
            "Reply with exactly: AGAIN_OK",
        ],
    )?;
    assert_eq!(again.code, Some(0), "{again}");
    assert!(
        project.task(2)?["title"]
            .as_str()
            .is_some_and(|title| title.contains("AGAIN_OK")),
        "T-2 is the chief's own: {}",
        project.task(2)?
    );
    rig.close()?;
    Ok(())
}
