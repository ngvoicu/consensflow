//! The daemon under load, opt-in (`npm run load`, which is `cargo xtask test
//! load`): several projects at once, each chief handing out waves of tasks to
//! fake workers in real PTYs through the real pane host, while the page polls
//! every board, task and transcript the whole time. At the end the daemon is
//! still there, every task is done and its result delivered, its log holds no
//! error and no slow pass, and the process did not grow past bounds. The size
//! scales with `CONSENSFLOW_LOAD_PROJECTS`, `CONSENSFLOW_LOAD_WAVES` and
//! `CONSENSFLOW_LOAD_TASKS` (tasks per wave per project).
//!
//! It is ignored unless asked for: `cargo test -p cf-e2e --test load -- --ignored`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::process::{own_var, Run};
use cf_e2e::rig::{Config, Project, Rig};
use cf_e2e::{files, Error};
use regex::Regex;
use serde_json::{json, Value};

type Outcome = Result<(), Box<dyn std::error::Error>>;

/// The stand-in for a window's harness program, built for these tests.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

const WORKERS: usize = 3;
const POLL: Duration = Duration::from_millis(50);
const RSS_LIMIT_MB: u64 = 512;

/// The size of the run: `name`'s value, or `default`.
fn size(name: &str, default: usize) -> usize {
    own_var(name)
        .and_then(|given| given.parse().ok())
        .unwrap_or(default)
}

/// The daemon's resident memory in MB, as the system counts it.
fn resident_mb(pid: u32) -> Result<u64, Box<dyn std::error::Error>> {
    let ran = Run::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .run()?;
    let kb: f64 = ran.stdout.trim().parse()?;
    Ok((kb / 1024.0).round() as u64)
}

#[test]
#[ignore = "opt-in: cargo xtask test load (npm run load)"]
fn the_daemon_stays_up_delivers_every_task_and_logs_nothing_wrong_while_several_projects_work_at_once(
) -> Outcome {
    let projects = size("CONSENSFLOW_LOAD_PROJECTS", 3);
    let waves = size("CONSENSFLOW_LOAD_WAVES", 3);
    let tasks = size("CONSENSFLOW_LOAD_TASKS", 3);
    let mut rig = Rig::start(Config::new(FAKE_AGENT))?;
    let workers: Vec<String> = (1..=WORKERS).map(|at| format!("worker{at}")).collect();
    let agents: Vec<Value> = std::iter::once("chief".to_owned())
        .chain(workers.iter().cloned())
        .map(|id| json!({ "id": id, "kind": "claude-code", "model": "fake" }))
        .collect();
    files::write(
        &rig.home().join("agents.json"),
        format!("{}\n", json!({ "schemaVersion": 1, "agents": agents })),
    )?;
    let mut opened = Vec::new();
    for at in 0..projects {
        let directory = rig.workspace().join(format!("project-{}", at + 1));
        files::make_dir(&directory)?;
        let staff: Vec<Value> = workers
            .iter()
            .map(|agent| json!({ "agent": agent, "roles": ["worker"] }))
            .collect();
        let project = Project::open(
            &rig,
            "chief",
            json!({ "directory": directory, "staff": staff }),
        )?;
        let board = project.board()?;
        let tier = board["lanes"]
            .as_array()
            .and_then(|lanes| {
                lanes
                    .iter()
                    .find(|lane| lane["participant"]["agent"] == workers[0].as_str())
            })
            .map(|lane| {
                lane["participant"]["tier"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .unwrap_or_default();
        opened.push((project.id(), tier));
    }
    let delivered = |id: i64| -> cf_e2e::Result<usize> {
        Ok(Project::new(&rig, id)
            .inbox("chief")?
            .iter()
            .filter(|m| m["kind"] == "result" && m["state"] == "delivered")
            .count())
    };

    // The page, reading everything all the time.
    let polling = AtomicBool::new(true);
    let polls = AtomicUsize::new(0);
    let poll_failures = Mutex::new(Vec::<String>::new());
    let seconds = thread::scope(|scope| -> Result<u64, Box<dyn std::error::Error>> {
        let page = scope.spawn(|| {
            while polling.load(Ordering::SeqCst) {
                for (id, _) in &opened {
                    let project = Project::new(&rig, *id);
                    let read = || -> cf_e2e::Result<()> {
                        let board = project.board()?;
                        let all: Vec<Value> = board["lanes"]
                            .as_array()
                            .map(|lanes| {
                                lanes
                                    .iter()
                                    .flat_map(|lane| {
                                        lane["tasks"].as_array().cloned().unwrap_or_default()
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        for task in &all[all.len().saturating_sub(2)..] {
                            rig.page("task.get", json!({ "project": id, "task": task["number"] }))?;
                            rig.page(
                                "task.transcript",
                                json!({ "project": id, "task": task["number"] }),
                            )?;
                        }
                        Ok(())
                    };
                    match read() {
                        Ok(()) => {
                            polls.fetch_add(1, Ordering::SeqCst);
                        }
                        Err(failed) => poll_failures
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(failed.to_string()),
                    }
                }
                thread::sleep(POLL);
            }
        });

        let started = Instant::now();
        let mut ran = Ok(());
        'waves: for wave in 0..waves {
            // Each project's chief is told its tasks at once with the others'.
            let told: Vec<cf_e2e::Result<()>> = thread::scope(|inner| {
                let handles: Vec<_> = opened
                    .iter()
                    .map(|(id, tier)| {
                        let rig = &rig;
                        inner.spawn(move || -> cf_e2e::Result<()> {
                            for n in 0..tasks {
                                rig.tell(
                                    *id,
                                    &format!(
                                        "DISPATCH --tier {tier} Reply with exactly: OK-{}-{}",
                                        wave + 1,
                                        n + 1
                                    ),
                                )?;
                            }
                            Ok(())
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle.join().unwrap_or_else(|_| {
                            Err(Error::Daemon("a chief was told in vain".into()))
                        })
                    })
                    .collect()
            });
            if let Some(failed) = told.into_iter().find_map(Result::err) {
                ran = Err(failed);
                break 'waves;
            }
            let expected = (wave + 1) * tasks;
            ran = rig.wait_for(
                &format!("every project to have {expected} results delivered"),
                Duration::from_secs(300),
                || {
                    for (id, _) in &opened {
                        if delivered(*id)? < expected {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                },
            );
            if ran.is_err() {
                break 'waves;
            }
        }
        polling.store(false, Ordering::SeqCst);
        let _ = page.join();
        ran?;
        Ok(started.elapsed().as_secs())
    })?;

    assert!(!rig.daemon_exited(), "the daemon is still running");
    let failures = poll_failures
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(failures, Vec::<String>::new(), "every poll was answered");
    for (id, _) in &opened {
        let board = Project::new(&rig, *id).board()?;
        let all: Vec<Value> = board["lanes"]
            .as_array()
            .map(|lanes| {
                lanes
                    .iter()
                    .flat_map(|lane| lane["tasks"].as_array().cloned().unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(all.len(), waves * tasks, "project {id} has every task");
        let undone: Vec<String> = all
            .iter()
            .filter(|t| t["state"] != "done")
            .map(|t| format!("T-{} {}", t["number"], t["state"]))
            .collect();
        assert_eq!(
            undone,
            Vec::<String>::new(),
            "project {id}: every task is done"
        );
    }
    let log = rig.log();
    let lines: Vec<&str> = log.lines().filter(|line| !line.is_empty()).collect();
    // Under the native daemon.
    assert!(
        Regex::new(r"^\S+ info start pid \d+ rust ")?
            .is_match(lines.first().copied().unwrap_or_default()),
        "the native daemon was under load"
    );
    let wrong: Vec<&&str> = lines
        .iter()
        .filter(|line| line.contains(" error ") || line.contains(" warn "))
        .collect();
    assert_eq!(
        wrong,
        Vec::<&&str>::new(),
        "no error and no slow pass under load"
    );
    let rss = resident_mb(rig.daemon_pid())?;
    assert!(
        rss < RSS_LIMIT_MB,
        "the daemon is {rss} MB, past {RSS_LIMIT_MB}"
    );
    println!(
        "load: {projects} projects × {waves} waves × {tasks} tasks = {} tasks in {seconds} s, {} board polls, daemon at {rss} MB",
        projects * waves * tasks,
        polls.load(Ordering::SeqCst)
    );
    rig.close()?;
    Ok(())
}

/// A window that prints as fast as it can, `CONSENSFLOW_LOAD_NOISE_MIB` of text (64 by
/// default; 256 for a noisy window of the long run's two hours), and the rig that holds what it
/// printed (T-139, the rig's capture was unbounded: every byte a window printed was kept
/// for the rig's life, and a failure's report made a string of each of its lines). What
/// this process holds once the window is done, and at its peak as the rig, dropped
/// unclosed, makes its report, stays under [`CAPTURE_LIMIT_MB`]: zeewin has 7 GB.
#[cfg(unix)]
#[test]
#[ignore = "opt-in: cargo xtask test load (npm run load)"]
fn a_window_that_prints_for_hours_holds_the_rig_and_its_report_to_a_bound() -> Outcome {
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    let mib = size("CONSENSFLOW_LOAD_NOISE_MIB", 64);
    let own = std::process::id();
    let peak = Arc::new(AtomicU64::new(0));
    let sampling = Arc::new(AtomicBool::new(true));
    let sampler = {
        let (peak, sampling) = (Arc::clone(&peak), Arc::clone(&sampling));
        thread::spawn(move || {
            while sampling.load(Ordering::SeqCst) {
                if let Ok(mb) = resident_mb(own) {
                    peak.fetch_max(mb, Ordering::SeqCst);
                }
                thread::sleep(Duration::from_millis(100));
            }
        })
    };
    let before = resident_mb(own)?;
    let rig = Rig::start(Config::new(FAKE_AGENT))?;
    let (daemon, host) = (rig.daemon_pid(), rig.host_pid());
    let done = rig.workspace().join("noise-done");
    let pane = json!({ "id": "noisy-1", "generation": 1 });
    let opened = rig.host(
        "pane.open",
        json!({
            "id": "noisy-1", "generation": 1, "cwd": rig.workspace(),
            "argv": [FAKE_AGENT, "--noise", (mib * 1024 * 1024).to_string(), done],
            "env": {}, "size": { "rows": 24, "cols": 80 },
        }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    let begun = Instant::now();
    let within = Duration::from_secs(60 + 2 * mib as u64);
    // The host has drawn the window's last line once it is on the host's screen.
    rig.wait_for("the window to print all it was asked to", within, || {
        Ok(done.exists()
            && rig
                .snapshot(&pane, 3)
                .is_some_and(|answer| answer["tail"].to_string().contains("the noise is over")))
    })?;
    let seconds = begun.elapsed().as_secs_f64();
    thread::sleep(Duration::from_secs(2));
    let held = resident_mb(own)?;
    let (daemon, host) = (resident_mb(daemon)?, resident_mb(host)?);
    let kept = rig.printed("noisy-1").len();
    let home = rig.workspace().parent().map(std::path::Path::to_path_buf);
    // A rig dropped unclosed makes its report of what its windows printed.
    drop(rig);
    thread::sleep(Duration::from_millis(300));
    sampling.store(false, Ordering::SeqCst);
    let _ = sampler.join();
    let peak = peak.load(Ordering::SeqCst).max(held);
    if let Some(home) = home {
        let name = home
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_file(home.with_file_name(format!("{name}-left.txt")));
    }
    println!(
        "capture: a window printed {mib} MiB in {seconds:.0} s; the rig kept {} MiB of it; this process went from {before} MB to {held} MB, and peaked at {peak} MB with the report; the daemon was at {daemon} MB and the pane host at {host} MB",
        kept / (1024 * 1024)
    );
    assert!(
        peak < CAPTURE_LIMIT_MB,
        "this process peaked at {peak} MB, past {CAPTURE_LIMIT_MB}"
    );
    Ok(())
}

/// How much this process may hold, a noisy window's rig and its report.
#[cfg(unix)]
const CAPTURE_LIMIT_MB: u64 = 128;
