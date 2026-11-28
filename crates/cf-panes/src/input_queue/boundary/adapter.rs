//! The head-specific half of the boundary tests: what a page document, the navigation, the
//! windows' writers and the person's screen look like to this head. The tests (one level up)
//! use only this, so that the same inputs and expected outputs run against the earlier head
//! (`5e1b6593b`, whose adapter maps a document to a page id and navigation to `end_page`).

use super::super::*;

/// A page document: the id it announces itself with, and the epoch the host answered it
/// (none from a head that has no epoch).
#[derive(Clone, Debug)]
pub(super) struct Doc {
    pub page: String,
    pub epoch: Option<u64>,
    nonce: String,
}

impl Doc {
    /// The same document, claiming another epoch (a call made up by a stale or wrong page).
    pub fn claiming(&self, epoch: u64) -> Doc {
        Doc {
            epoch: Some(epoch),
            ..self.clone()
        }
    }

    /// A page the host never prepared, at an epoch it claims.
    pub fn unbound(page: &str, epoch: Option<u64>) -> Doc {
        Doc {
            page: page.to_owned(),
            epoch,
            nonce: "unbound".to_owned(),
        }
    }

    fn at(&self) -> u64 {
        self.epoch.unwrap_or(0)
    }
}

pub(super) use super::super::{Notice, NoticeBody};

/// A window with a writer nobody drains but the test: what is handed to it, in order.
pub(super) struct Window {
    pub key: PaneKey,
    jobs: mpsc::Receiver<InputJob>,
    _replies: mpsc::Receiver<InputJob>,
    pending: Arc<AtomicUsize>,
}

pub(super) struct Rig {
    pub q: InputQueue,
}

/// A call the host handed to a window's writer, which the test finishes.
pub(super) struct Job(InputJob);

impl Job {
    pub fn bytes(&self) -> &[u8] {
        match &self.0.work {
            InputWork::Typed(part) | InputWork::Reply(part) | InputWork::Write(part) => part,
            InputWork::Paste { body, .. } => body,
            InputWork::Claim | InputWork::Enter(_) => &[],
        }
    }

    fn finish(self, result: InputResponse) {
        let InputJob {
            response, outcome, ..
        } = self.0;
        match outcome {
            Some(outcome) => outcome.finish(result, response),
            None => {
                let _ = response.send(result);
            }
        }
    }

    pub fn written(self) {
        self.finish(Ok(()));
    }

    pub fn failed(self, error: InputError) {
        self.finish(Err(error));
    }

    /// The writer disappears with the call, answering nothing.
    pub fn vanishes(self) {
        drop(self.0);
    }
}

pub(super) type Refused = (String, Option<u64>);

impl Rig {
    pub fn new() -> Self {
        let q = InputQueue::new(
            Arc::new(PaneTable::new()),
            Arc::new(InputArbiter::new(crate::arbiter::EnterTiming::fixed(0))),
        );
        Self { q }
    }

    /// A window whose writer queue holds what the host hands it (1,024 calls).
    pub fn window(&self, id: &str, generation: u64) -> Window {
        let key = PaneKey::new(id, generation);
        let (sender, jobs) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let (replies, replied) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let pending = Arc::new(AtomicUsize::new(0));
        self.q.senders.lock().unwrap().insert(
            key.clone(),
            InputRoute {
                alive: Arc::new(AtomicBool::new(true)),
                sender,
                replies,
                pending_bytes: Arc::clone(&pending),
                reply_bytes: Arc::new(AtomicUsize::new(0)),
            },
        );
        Window {
            key,
            jobs,
            _replies: replied,
            pending,
        }
    }

    /// A new main document begins loading.
    pub fn navigate(&self) {
        self.q.end_page().unwrap();
    }

    pub fn prepare(&self, page: &str) -> Doc {
        self.try_prepare(page).unwrap()
    }

    pub fn try_prepare(&self, page: &str) -> Result<Doc, String> {
        let nonce = format!("nonce-of-{page}");
        let epoch = self.q.prepare_page(page, &nonce)?;
        Ok(Doc {
            page: page.to_owned(),
            epoch: Some(epoch),
            nonce,
        })
    }

    pub fn activate(&self, doc: &Doc) -> Result<Vec<Notice>, String> {
        self.q.activate_page(&doc.page, &doc.nonce, doc.at())
    }

    pub fn open_page(&self, page: &str) -> Doc {
        let doc = self.prepare(page);
        self.activate(&doc).unwrap();
        doc
    }

    pub fn input(
        &self,
        doc: &Doc,
        window: &Window,
        number: u64,
        bytes: &[u8],
    ) -> Result<String, Refused> {
        self.input_work(doc, window, number, InputWork::Typed(bytes.to_vec()))
    }

    pub fn reply(
        &self,
        doc: &Doc,
        window: &Window,
        number: u64,
        bytes: &[u8],
    ) -> Result<String, Refused> {
        self.input_work(doc, window, number, InputWork::Reply(bytes.to_vec()))
    }

    fn input_work(
        &self,
        doc: &Doc,
        window: &Window,
        number: u64,
        work: InputWork,
    ) -> Result<String, Refused> {
        self.q
            .enqueue_page_told(&doc.page, doc.at(), window.key.clone(), number, work, true)
            .map_err(|refusal| (refusal.error, refusal.loss))
    }

    /// A call for a window by its key (one the test made no writer for, or whose writer is gone).
    pub fn input_for(
        &self,
        doc: &Doc,
        key: &PaneKey,
        number: u64,
        bytes: &[u8],
    ) -> Result<String, Refused> {
        self.q
            .enqueue_page_told(
                &doc.page,
                doc.at(),
                key.clone(),
                number,
                InputWork::Typed(bytes.to_vec()),
                true,
            )
            .map_err(|refusal| (refusal.error, refusal.loss))
    }

    pub fn skip(&self, doc: &Doc, window: &Window, number: u64) -> Result<u64, String> {
        self.skip_key(doc, &window.key, number)
    }

    pub fn skip_key(&self, doc: &Doc, key: &PaneKey, number: u64) -> Result<u64, String> {
        self.q.skip_page_sequence(&doc.page, doc.at(), key, number)
    }

    /// The window's route is gone from the host (its pane left the table).
    pub fn remove_route(&self, key: &PaneKey) {
        self.q.senders.lock().unwrap().remove(key);
    }

    /// A call whose bytes did not decode: refused before the host saw them.
    pub fn refuse_decode(&self, doc: &Doc, window: &Window, number: u64) -> Result<u64, String> {
        self.q
            .refuse_page_sequence(&doc.page, doc.at(), &window.key, number)
    }

    /// What the host still owes the person, as this document asks it.
    pub fn pull(&self, doc: &Doc) -> Result<Vec<Notice>, String> {
        self.q.pending_losses(&doc.page, doc.at())
    }

    /// The person dismisses the notice with this id, from this document.
    pub fn dismiss(&self, doc: &Doc, id: u64) -> Result<bool, String> {
        self.q.dismiss_notice(&doc.page, doc.at(), id)
    }

    /// The page says what a window is called.
    pub fn name(&self, doc: &Doc, key: &PaneKey, name: &str) -> Result<(), String> {
        self.q.name_window(&doc.page, doc.at(), key, name)
    }

    /// A drop's place: the number taken at the gesture, told to the host.
    pub fn place(&self, doc: &Doc, window: &Window, number: u64) -> Result<(), String> {
        self.q
            .place_page_sequence(&doc.page, doc.at(), &window.key, number)
    }

    /// A place nobody used, let go.
    pub fn release(&self, doc: &Doc, window: &Window, number: u64) -> Result<(), String> {
        self.q
            .release_page_sequence(&doc.page, doc.at(), &window.key, number)
    }

    /// What was handed to the window's writer since the last call, in order; the writer
    /// writes it, and answers each call.
    pub fn written(&self, window: &Window) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Ok(job) = window.jobs.try_recv() {
            match &job.work {
                InputWork::Typed(part) | InputWork::Reply(part) | InputWork::Write(part) => {
                    bytes.extend_from_slice(part)
                }
                InputWork::Paste { body, .. } => bytes.extend_from_slice(body),
                InputWork::Claim | InputWork::Enter(_) => {}
            }
            let InputJob {
                response, outcome, ..
            } = job;
            match outcome {
                Some(outcome) => outcome.finish(Ok(()), response),
                None => {
                    let _ = response.send(Ok(()));
                }
            }
        }
        bytes
    }

    /// The window's writer, which the test drives: take what it was handed next, and finish
    /// it with the result a worker would give.
    pub fn writer_finishes(&self, window: &Window, result: InputResponse) {
        let job = window
            .jobs
            .try_recv()
            .expect("a call was handed to the writer");
        let InputJob {
            response, outcome, ..
        } = job;
        match outcome {
            Some(outcome) => outcome.finish(result, response),
            None => {
                let _ = response.send(result);
            }
        }
    }

    /// The next call handed to the window's writer, if one waits.
    pub fn take(&self, window: &Window) -> Option<Job> {
        window.jobs.try_recv().ok().map(Job)
    }

    /// The window's writer disappears with the call it took, answering nothing.
    pub fn writer_vanishes(&self, window: &Window) {
        drop(
            window
                .jobs
                .try_recv()
                .expect("a call was handed to the writer"),
        );
    }

    /// The window's writer queue is gone altogether.
    pub fn writer_queue_gone(&self, window: Window) -> PaneKey {
        let Window { key, jobs, .. } = window;
        drop(jobs);
        key
    }

    /// What was handed to the window's replies' writer.
    pub fn replied(&self, window: &Window) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Ok(job) = window._replies.try_recv() {
            if let InputWork::Reply(part) = &job.work {
                bytes.extend_from_slice(part);
            }
        }
        bytes
    }

    /// The bytes the window holds reserved for the person's pending input.
    pub fn reserved(&self, window: &Window) -> usize {
        window.pending.load(Ordering::Acquire)
    }

    pub fn retire(&self, window: &Window) {
        self.q.retire(&window.key);
    }

    /// The wait for a call's ticket, if its answer is there: what the page would be told.
    pub fn wait(&self, doc: &Doc, ticket: &str) -> Option<Result<InputResponse, String>> {
        let completion = match self.q.page_completion(&doc.page, ticket) {
            Ok(completion) => completion,
            Err(error) => return Some(Err(error)),
        };
        completion.receiver.now_or_never().map(Ok)
    }

    /// The wait for a ticket, as the page's `pane_input_wait` holds it.
    pub fn completion(&self, doc: &Doc, ticket: &str) -> PageInputCompletion {
        self.q.page_completion(&doc.page, ticket).unwrap()
    }

    /// The host's state lock, held by the test: a worker that goes meanwhile finds it busy.
    pub fn hold_state(&self) -> MutexGuard<'_, PageInputState> {
        self.q.page.state.lock().unwrap()
    }

    /// A caught panic with the host's state held: the lock is poisoned.
    pub fn poison_state(&self) {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _held = self.q.page.state.lock().unwrap();
            panic!("a test panics holding the host's state");
        }));
        assert!(outcome.is_err());
    }

    /// The host's bounds clock is the test's: nothing starts a thread, and `bounds_pass` is time.
    pub fn manual_clock(&self) {
        self.q.page.state.lock().unwrap().reaper = true;
    }

    /// The cap on outstanding notices, made small so that a schedule reaches a fold.
    pub fn notice_cap(&self, cap: usize) {
        self.q.page.state.lock().unwrap().notice_cap = cap;
    }

    /// Every bound the parked calls wait on has passed.
    pub fn bounds_pass(&self) {
        let mut events = Vec::new();
        self.q.page.lock().pump_all(
            Instant::now() + ORDER_BOUND + Duration::from_millis(5),
            &mut events,
        );
        self.q.page.flush(events);
    }

    /// For the schedules' checks: the notice each loss number made, and each notice's successor.
    pub fn ledger(&self) -> (HashMap<u64, u64>, HashMap<u64, u64>) {
        let state = self.q.page.lock();
        (state.covers.clone(), state.lineage.clone())
    }

    /// The page identities the host holds after its cuts.
    pub fn held_identities(&self) -> usize {
        self.q.held_identities()
    }

    /// The next thread the host starts (its bounds clock, its outcome helper) fails to start.
    pub fn refuse_threads(&self, refuse: bool) {
        super::super::FAIL_THREAD_SPAWN.with(|fail| fail.set(refuse));
    }

    /// Whether this document may subscribe to the windows' output.
    pub fn may_subscribe(&self, doc: &Doc) -> bool {
        self.q.with_current_page(&doc.page, || ()).is_ok()
    }

    pub fn host_epoch(&self) -> Option<u64> {
        Some(self.q.epoch())
    }
}
