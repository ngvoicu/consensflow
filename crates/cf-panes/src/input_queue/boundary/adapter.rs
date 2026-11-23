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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Notice {
    pub key: PaneKey,
    pub loss: u64,
    pub closed: bool,
}

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
        self.q
            .activate_page(&doc.page, &doc.nonce, doc.at())
            .map(|notices| notices_of(&notices))
    }

    pub fn open_page(&self, page: &str) -> Doc {
        let doc = self.prepare(page);
        self.activate(&doc).unwrap();
        doc
    }

    pub fn input(&self, doc: &Doc, window: &Window, number: u64, bytes: &[u8]) -> Result<String, Refused> {
        self.input_work(doc, window, number, InputWork::Typed(bytes.to_vec()))
    }

    pub fn reply(&self, doc: &Doc, window: &Window, number: u64, bytes: &[u8]) -> Result<String, Refused> {
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
        self.q
            .pending_losses(&doc.page, doc.at())
            .map(|notices| notices_of(&notices))
    }

    /// The count past the host's bound, with its version, as this document asks it.
    pub fn overflow(&self, doc: &Doc) -> Option<(u64, u64)> {
        self.q
            .pending_losses(&doc.page, doc.at())
            .unwrap()
            .overflow
            .map(|overflow| (overflow.version, overflow.count))
    }

    /// This document says it has shown these notices.
    pub fn told(&self, doc: &Doc, shown: &[(PaneKey, u64)]) -> Result<(), String> {
        self.q.lost_told(&doc.page, doc.at(), shown, None)
    }

    pub fn told_overflow(&self, doc: &Doc, version: u64, count: u64) -> Result<(), String> {
        self.q
            .lost_told(&doc.page, doc.at(), &[], Some((version, count)))
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
                InputWork::Claim => {}
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
        let job = window.jobs.try_recv().expect("a call was handed to the writer");
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

    /// The window's writer disappears with the call it took, answering nothing.
    pub fn writer_vanishes(&self, window: &Window) {
        drop(window.jobs.try_recv().expect("a call was handed to the writer"));
    }

    /// The window's writer queue is gone altogether.
    pub fn writer_queue_gone(&self, window: Window) -> PaneKey {
        let Window { key, jobs, .. } = window;
        drop(jobs);
        key
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

    /// Whether this document may subscribe to the windows' output.
    pub fn may_subscribe(&self, doc: &Doc) -> bool {
        self.q.with_current_page(&doc.page, || ()).is_ok()
    }

    pub fn host_epoch(&self) -> Option<u64> {
        Some(self.q.epoch())
    }
}

fn notices_of(notices: &PendingNotices) -> Vec<Notice> {
    notices
        .losses
        .iter()
        .map(|notice| Notice {
            key: notice.key.clone(),
            loss: notice.loss,
            closed: notice.closed,
        })
        .collect()
}
