//! A trace played against the API under test, step by step, as `FORMAT.md`
//! says: the ledger's calls replayed, the windows' tokens issued, each
//! exchange sent and its answer compared as bytes (with the wake-ups, the
//! events, the clock and the roster it drew on), each run of `cf` made whole
//! and what it printed compared, and at the end the database the ledger left.
//!
//! An exchange or a run that other steps overlap (a door held open while the
//! API closes; a hook's long poll that the test answers meanwhile) is left
//! running until its `settle`. Before any exchange of the test's own, the API
//! has taken every request recorded before it: a run's requests arrive when
//! `cf` makes them.

use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use base64::Engine;
use serde_json::Value;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::checks;
use crate::front::{self, Reply};
use crate::names::Names;
use crate::rig::Rig;
use crate::runs::{self, Ran};
use crate::support::compare::differs;
use crate::support::trace::{self, Tally};

/// How long a step may wait for what it needs of the API or of `cf`.
const WAIT: Duration = Duration::from_secs(60);

/// A run of `cf` still going: what it was, and what it makes of the API.
struct Running {
    step: Value,
    exchanges: Vec<Value>,
    ran: oneshot::Receiver<Ran>,
}

/// An exchange of the test's own still waiting for its answer.
struct Waiting {
    step: Value,
    reply: JoinHandle<io::Result<Reply>>,
}

struct Player {
    rig: Rc<Rig>,
    names: Names,
    trace: Value,
    /// The exchanges the runs make, by run, in order.
    made: HashMap<u64, Vec<Value>>,
    runs: HashMap<u64, Running>,
    exchanges: HashMap<u64, Waiting>,
    closes: HashMap<u64, JoinHandle<()>>,
    /// What the API did that belongs to what is still running.
    events: Vec<Value>,
    kicks: usize,
    /// What has been compared so far.
    tally: Tally,
}

/// Plays the trace called `name`: what it held of it, or why it was not
/// answered as Node answered.
pub async fn play(name: &str) -> Result<Tally, String> {
    let rig = Rc::new(Rig::start().await);
    let trace = trace::load(name);
    let steps = trace["steps"].as_array().cloned().unwrap_or_default();
    let mut made: HashMap<u64, Vec<Value>> = HashMap::new();
    for step in &steps {
        if step["kind"] == "exchange" && step["client"] == "cf" {
            made.entry(step["run"].as_u64().expect("the run it belongs to"))
                .or_default()
                .push(step.clone());
        }
    }
    let mut player = Player {
        names: Names::new(&front::address(&rig.api)),
        rig,
        trace,
        made,
        runs: HashMap::new(),
        exchanges: HashMap::new(),
        closes: HashMap::new(),
        events: Vec::new(),
        kicks: 0,
        tally: Tally::default(),
    };
    for (at, step) in steps.iter().enumerate() {
        let kind = step["kind"].as_str().unwrap_or("?");
        player
            .step(kind, step)
            .await
            .map_err(|why| format!("{name}, step {at} ({kind}): {why}"))?;
    }
    player
        .finish()
        .map_err(|why| format!("{name}, at its end: {why}"))?;
    player.tally.traces += 1;
    Ok(player.tally)
}

impl Player {
    async fn step(&mut self, kind: &str, step: &Value) -> Result<(), String> {
        match kind {
            "ledger" => self.ledger(step),
            "issue" => {
                let participant = step["participant"]["id"].as_i64().expect("a participant");
                let token = self
                    .rig
                    .front
                    .context
                    .credentials
                    .issue(step["project"].as_i64().expect("a project"), participant);
                self.names
                    .issued(step["token"].as_str().expect("a name"), token);
                Ok(())
            }
            "revoke" => {
                let token = self.names.token(step["token"].as_str().expect("a token"));
                self.rig.front.context.credentials.revoke(&token);
                Ok(())
            }
            "exchange" => self.exchange(step).await,
            "run" => self.run(step).await,
            "api.close" => {
                let id = step["id"].as_u64().expect("an id");
                let rig = Rc::clone(&self.rig);
                let closing = tokio::task::spawn_local(async move { rig.api.close().await });
                if step["detached"] == true {
                    self.closes.insert(id, closing);
                    return Ok(());
                }
                closing
                    .await
                    .map_err(|why| format!("the close failed: {why}"))
            }
            "settle" => self.settle(step).await,
            other => Err(format!("the player does not play a step of {other}")),
        }
    }

    /// A call the test made on its ledger.
    fn ledger(&mut self, step: &Value) -> Result<(), String> {
        self.hold()?;
        if step["method"] != "close" {
            return self.rig.ledger.apply(step).map_or(Ok(()), Err);
        }
        if let Some(why) = self.rig.ledger.close(step, &self.trace["ledger"]["final"]) {
            return Err(why);
        }
        self.tally.databases += 1;
        Ok(())
    }

    /// An exchange of the test's own: sent here. One that is a run's is the
    /// run's to make.
    async fn exchange(&mut self, step: &Value) -> Result<(), String> {
        if step["client"] == "cf" {
            return Ok(());
        }
        let id = step["id"].as_u64().expect("an id");
        self.until_received(id - 1).await?;
        self.hold()?;
        self.rig.ledger.give(step);
        self.save_agents(std::slice::from_ref(step));
        let sending = self.send(step);
        if step["detached"] == true {
            self.exchanges.insert(
                id,
                Waiting {
                    step: step.clone(),
                    reply: sending,
                },
            );
            return self.until_received(id).await;
        }
        let reply = sending
            .await
            .map_err(|why| format!("the exchange failed: {why}"))?
            .map_err(|why| format!("no answer: {why}"))?;
        let (events, kicks) = self.take();
        checks::exchange(&self.rig, step, &reply, &events, kicks)?;
        self.tally.exchanges += 1;
        Ok(())
    }

    /// Sends the request of an exchange, as the trace has it.
    fn send(&self, step: &Value) -> JoinHandle<io::Result<Reply>> {
        let request = &step["request"];
        let method = request["method"].as_str().expect("a method").to_owned();
        let target = self
            .names
            .put(request["target"].as_str().expect("a target"));
        let authorization = request["authorization"]
            .as_str()
            .map(|header| self.names.put(header));
        let content_type = request["contentType"].as_str().map(str::to_owned);
        let body: Option<Vec<u8>> = match (request["bodyBase64"].as_str(), request["body"].as_str())
        {
            (Some(bytes), _) => Some(
                base64::engine::general_purpose::STANDARD
                    .decode(bytes)
                    .expect("a body in base64"),
            ),
            (None, Some(text)) => Some(text.as_bytes().to_vec()),
            (None, None) => None,
        };
        let address = front::address(&self.rig.api);
        tokio::task::spawn_local(async move {
            front::send(
                &address,
                front::Request {
                    method: &method,
                    target: &target,
                    authorization: authorization.as_deref(),
                    content_type: content_type.as_deref(),
                    body: body.as_deref(),
                },
            )
            .await
        })
    }

    /// The saved agents the roster stand-in of these exchanges answered with.
    fn save_agents(&self, exchanges: &[Value]) {
        let rows: Vec<Value> = exchanges
            .iter()
            .flat_map(|exchange| exchange["seams"].as_array().cloned().unwrap_or_default())
            .map(|call| call["result"].clone())
            .filter(|row| !row.is_null())
            .collect();
        if !rows.is_empty() {
            self.rig.save_agents(&rows);
        }
    }

    /// One run of `cf`, made whole.
    async fn run(&mut self, step: &Value) -> Result<(), String> {
        let id = step["id"].as_u64().expect("an id");
        let exchanges = self.made.get(&id).cloned().unwrap_or_default();
        self.hold()?;
        for exchange in &exchanges {
            self.rig.ledger.give(exchange);
        }
        self.save_agents(&exchanges);
        let strings = |key: &str| -> Vec<String> {
            step[key]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let env: Vec<(String, String)> = step["env"]
            .as_object()
            .map(|env| {
                env.iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            self.names.put(value.as_str().unwrap_or_default()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let stdin = step["stdin"].as_str().unwrap_or_default().to_owned();
        let ran = runs::start(strings("argv"), env, stdin);
        let running = Running {
            step: step.clone(),
            exchanges,
            ran,
        };
        if step["detached"] == true {
            self.runs.insert(id, running);
            return Ok(());
        }
        self.finish_run(running).await
    }

    /// What a run printed and how it ended, and what it made of the API.
    async fn finish_run(&mut self, running: Running) -> Result<(), String> {
        let Running {
            step,
            exchanges,
            ran,
        } = running;
        let ran = tokio::time::timeout(WAIT, ran)
            .await
            .map_err(|_| "cf did not end".to_owned())?
            .map_err(|_| "cf was lost".to_owned())?;
        let stdout = step["stdout"].as_str().unwrap_or_default();
        if let Some(why) = differs("its output", &ran.stdout, stdout) {
            return Err(why);
        }
        let stderr = step["stderr"].as_str().unwrap_or_default();
        if let Some(why) = differs("its error output", &ran.stderr, stderr) {
            return Err(why);
        }
        if ran.code.map(i64::from) != step["code"].as_i64() {
            return Err(format!("ended with {:?}, Node {}", ran.code, step["code"]));
        }
        self.tally.runs += 1;
        for exchange in &exchanges {
            checks::made(&self.rig, &self.names, exchange)?;
            self.tally.exchanges += 1;
        }
        let (events, kicks) = self.take_all();
        let all: Vec<&Value> = exchanges.iter().collect();
        checks::effects(&self.rig, &all, &events, kicks)
    }

    /// What was left running comes to its end, here.
    async fn settle(&mut self, step: &Value) -> Result<(), String> {
        if let Some(id) = step["exchange"].as_u64() {
            let Some(waiting) = self.exchanges.remove(&id) else {
                // A run's exchange: the run settles it.
                return Ok(());
            };
            let reply = waiting
                .reply
                .await
                .map_err(|why| format!("the exchange failed: {why}"))?
                .map_err(|why| format!("no answer: {why}"))?;
            let (events, kicks) = self.take_all();
            checks::exchange(&self.rig, &waiting.step, &reply, &events, kicks)?;
            self.tally.exchanges += 1;
            return Ok(());
        }
        if let Some(id) = step["close"].as_u64() {
            let closing = self
                .closes
                .remove(&id)
                .ok_or(format!("close {id} was never begun"))?;
            return closing
                .await
                .map_err(|why| format!("the close failed: {why}"));
        }
        let id = step["run"].as_u64().expect("something to settle");
        let running = self
            .runs
            .remove(&id)
            .ok_or(format!("run {id} was never begun"))?;
        self.finish_run(running).await
    }

    /// Waits until the API has taken `count` requests.
    async fn until_received(&self, count: u64) -> Result<(), String> {
        let count = usize::try_from(count).expect("a small count");
        let waited = tokio::time::timeout(WAIT, async {
            while self.rig.received() < count {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await;
        waited.map_err(|_| {
            format!(
                "the API took {} requests, where Node's exchange came after {count}",
                self.rig.received()
            )
        })
    }

    /// Everything the API did so far belongs to what is still running; and
    /// nothing may have been done that nothing is running for.
    fn hold(&mut self) -> Result<(), String> {
        let (events, kicks) = self.take();
        self.events.extend(events);
        self.kicks += kicks;
        let running = !self.runs.is_empty() || !self.exchanges.is_empty();
        if !running && (!self.events.is_empty() || self.kicks > 0) {
            return Err(format!(
                "the API logged {:?} and woke the dispatcher {} times, with nothing asking it to",
                self.events, self.kicks
            ));
        }
        Ok(())
    }

    /// What the API did since it was last asked.
    fn take(&self) -> (Vec<Value>, usize) {
        (self.rig.ledger.take_events(), self.rig.front.take_kicks())
    }

    /// What it did, with what was held for what has run.
    fn take_all(&mut self) -> (Vec<Value>, usize) {
        let (events, kicks) = self.take();
        let mut all = std::mem::take(&mut self.events);
        all.extend(events);
        let kicks = kicks + std::mem::take(&mut self.kicks);
        (all, kicks)
    }

    /// The trace is played: nothing is left running and nothing is left over.
    fn finish(&mut self) -> Result<(), String> {
        if !self.runs.is_empty() || !self.exchanges.is_empty() || !self.closes.is_empty() {
            return Err("something was left running".to_owned());
        }
        if self.trace["ledger"].is_null() {
            // A trace with no ledger of its own never closed this one: its
            // file is let go here, where it is held open until the home goes.
            self.rig
                .ledger
                .ledger
                .borrow_mut()
                .close_in_place()
                .map_err(|why| format!("the ledger did not close: {why}"))?;
        }
        self.hold()
    }
}
