//! The engine's record of a participant (`runtime`, `dispatcher.js`), made
//! the first time it is asked for and forgotten when the participant leaves.
//! Each part has one owner: the window itself (`windows`), what is on its
//! way into it (`deliveries`), what its harness said of its quota
//! (`scheduler`), what of its conversation is copied (`transcripts`), and a
//! Switch chief waiting for the chief's turn to end (`chief_switch`); each
//! module adds to its part as it is ported. Who holds
//! the participant now is its [`Hold`]. A part is borrowed for one reading
//! or one change, never across a wait.

use std::cell::RefCell;
use std::rc::Rc;

use crate::chief_switch::PendingSwitch;
use crate::deliveries::DeliveryPart;
use crate::runtime::Hold;
use crate::scheduler::QuotaPart;
use crate::transcripts::Copied;
use crate::windows::WindowPart;

/// One participant's record.
pub(crate) struct Record {
    /// The participant's id in the ledger.
    pub(crate) id: i64,
    pub(crate) hold: Rc<Hold>,
    pub(crate) window: RefCell<WindowPart>,
    pub(crate) delivery: RefCell<DeliveryPart>,
    pub(crate) quota: RefCell<QuotaPart>,
    pub(crate) copied: RefCell<Option<Copied>>,
    pub(crate) pending_switch: RefCell<Option<PendingSwitch>>,
}

impl Record {
    /// A participant's record as it is made: no window, nothing on its way.
    pub(crate) fn new(id: i64) -> Rc<Self> {
        Rc::new(Self {
            id,
            hold: Rc::new(Hold::default()),
            window: RefCell::new(WindowPart::default()),
            delivery: RefCell::new(DeliveryPart::default()),
            quota: RefCell::new(QuotaPart::default()),
            copied: RefCell::new(None),
            pending_switch: RefCell::new(None),
        })
    }
}
