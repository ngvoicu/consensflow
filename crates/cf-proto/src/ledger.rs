//! What the ledger answers, as it crosses to the API and the page: each view
//! with its fields in the order the Node ledger wrote them, so its JSON reads
//! as it did.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::specs::{AppSpecs, SpecsSetting, SpecsView};

/// A participant of a project: the human, the chief, a member, or a member's session.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantView {
    pub id: i64,
    pub project_id: i64,
    pub handle: String,
    pub role: String,
    /// The ready-made agent a participant from before the models were the harnesses' own
    /// ran on (spec `harness-models`): the name it had, which history shows. None for
    /// every participant since.
    pub agent: Option<String>,
    pub harness: Option<String>,
    /// The model it runs on, as its harness takes it (Pi's `provider/id`, Devin's exact
    /// `model_uid`): a member's, and the chief's, which it runs on where its record names
    /// none (CHF-04). None for a member on its harness's default. Said only where there is
    /// one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The effort it runs its model at. Said only where there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The model as its harness's list names it, its id (`claude-opus-5-5`; Pi's
    /// `provider/id`; Devin's listed id, which its `model` does not hold once an effort is
    /// part of it): what it is shown as, beside its effort. Said only where there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    /// Every name the participant went by before this one, the oldest first: a member renamed
    /// by its model's id (`opus-high`), and its windows. History says them with `agent`.
    /// Said only where there is one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub former_handles: Vec<String>,
    /// A member's, or a window's, name that holds no model and effort of the ones it runs
    /// on (a designer on its harness's default, a member the update could not tell the model
    /// of): what it runs on is said beside it. Never said of the chief and the human; said
    /// only where it is so.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub name_apart: bool,
    /// A member from before the models were the harnesses' own whose agent the update
    /// could not resolve (T-1202): it keeps the name it had and runs on nothing until the
    /// human edits it. Said only where it is so.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub needs_edit: bool,
    /// A chief's: the model its last launch asked its harness for, read from its record
    /// or picked; none where it left it to the harness's default. Not said: the board
    /// says it of the chief's lane.
    #[serde(skip_serializing)]
    pub launched_model: Option<String>,
    /// A chief's: the model a launch found its harness no longer has, which a reopen never
    /// asks for again though its record names it (CHF-06). Not said.
    #[serde(skip_serializing)]
    pub gone_model: Option<String>,
    /// A chief's: the model its last launch ran on, where it is not the one picked for it
    /// (the human switched it in the window, and the reopen kept it): what its card says
    /// it runs on (O7). Said only then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ran_on: Option<String>,
    pub designer: bool,
    pub created_at: String,
    pub left_at: Option<String>,
    pub tier: Option<String>,
    pub roles: Vec<String>,
    pub out_until: Option<String>,
    pub out_since: Option<String>,
    pub member_id: Option<i64>,
    /// The member a session belongs to.
    pub member: Option<String>,
    /// A session's own name, after its member's handle.
    pub session: Option<String>,
}

/// A project with the participants still in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: i64,
    pub directory: String,
    pub name: String,
    pub state: String,
    pub gate: bool,
    pub created_at: String,
    pub updated_at: String,
    pub participants: Vec<ParticipantView>,
    /// What the project says of specs on its own: said only when it says
    /// anything, so a project that never did reads as it always did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub specs: Option<SpecsView>,
    /// The line the human wrote under the project's name, for them alone: said
    /// only when there is one, so a project that has none reads as it always did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When the human began a meeting with the chief, while one is on: said only
    /// then, so a project with none reads as it always did (3.0.7 chief-meeting).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meeting: Option<String>,
    /// The rules the human wrote for the project's staff, which every member's
    /// window is given after its role, and the chief's as what every window opens with:
    /// said only when there are some, so a project that has none reads as it
    /// always did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staff_rules: Option<String>,
    /// The size, in tokens, at which the human has the project's member windows
    /// compact, whatever their harness: each member's launch gives it its harness by
    /// that harness's own rule. Said only when the human chose one, so a project on
    /// each harness's own point reads as it always did. The chief's window is never
    /// given it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compaction: Option<u32>,
    /// Whether the human has the project's members run with Claude Code's own advisor tool
    /// (Project settings, "Advisor tool"): said only when on, so a project with it off, which
    /// every project is until the human ticks it, reads as it always did. The chief's window is
    /// never given the switch, whatever this says.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub advisor_tool: bool,
    /// Whether the human leaves Claude Code's prompt suggestion on in the project's windows,
    /// the chief's and the members' (Project settings, "Prompt suggestion"): said only when on,
    /// so a project with it off, which every project is until the human ticks it, reads as it
    /// always did. Off, every Claude Code window of the project is launched with it switched
    /// off; on, ConsensFlow writes nothing about it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub prompt_suggestion: bool,
}

impl ProjectView {
    /// What its chief's launch is given of specs, where `app` is what the
    /// human says for every project: the project's own word, else the app's,
    /// else the documented folder and variant; none where specs are off for
    /// this one.
    pub fn specs_setting(&self, app: &AppSpecs) -> Option<SpecsSetting> {
        SpecsView::resolve(self.specs.as_ref(), app)
    }
}

/// What a deleted project took with it, for the one line the trace keeps.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedProject {
    pub id: i64,
    pub name: String,
    pub directory: String,
    pub created_at: String,
    pub members: i64,
    pub sessions: i64,
    pub tasks: i64,
    pub messages: i64,
}

/// One event of a project's log.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventView {
    pub id: i64,
    pub project_id: i64,
    pub at: String,
    pub kind: String,
    pub data: Value,
}

/// A member who left the staff, and the tasks cancelled with it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemovedMember {
    pub member: ParticipantView,
    pub cancelled: Vec<i64>,
}

/// A member of the newest project's staff, as a new project starts from it: the harness,
/// the model, the effort and the tier it runs on, and its roles.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaffMember {
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub model_name: Option<String>,
    pub designer: bool,
    pub tier: Option<String>,
    pub role: String,
    pub roles: Vec<String>,
}

/// An active member of one role, with what the daemon ranks it by.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    pub id: i64,
    pub handle: String,
    pub agent: Option<String>,
    pub harness: Option<String>,
    pub tier: Option<String>,
    pub roles: Vec<String>,
    /// How many tasks it and its sessions have taken.
    pub taken: i64,
    /// How many of its windows have a task on their hands now.
    pub sessions: i64,
    pub out_until: Option<String>,
    /// Whether the update could not tell what it runs on (T-1202): it takes no task until
    /// the human edits it. Said only where it is so.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub needs_edit: bool,
}

/// A member an open task may go to, and whether the task was taken from it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    #[serde(flatten)]
    pub member: MemberView,
    pub had_it: bool,
}

/// A participant's native conversation: the harness it runs on, the
/// harness's own session once it is known, and when it started and ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    pub id: i64,
    pub participant_id: i64,
    pub harness: String,
    pub native_session: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// One item of the copy ConsensFlow keeps of a conversation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryItem {
    pub id: String,
    pub role: String,
    pub text: String,
    pub complete: bool,
    pub at: Option<String>,
    /// The summary a compaction wrote in the user's place: a `user` item that
    /// is the harness's own words, not the human's. Said only when true: an
    /// item copied before the mark (migration 0020) is a plain `user` one.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compaction: bool,
    /// What the history draws of it besides its words, where its reader said it: none for
    /// an item copied before the copy kept it (migration 0032), which draws as a plain one,
    /// of its role.
    #[serde(flatten)]
    pub detail: Option<ItemDetail>,
}

/// What the history draws of an item besides its words (chief-harness CHF-16): its kind
/// (`prompt`, `text`, `thinking`, `tool`, `compaction`, `notice`); for a tool's call its
/// name, its header (256 characters at most), how it stands (`running`, `ok`, `failed`),
/// how long it took in milliseconds and the change it made as diff lines; for a compaction
/// the tokens it compacted from. Each said only where its reader said it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// An earlier conversation of the chief, with its copied items in order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChiefConversation {
    #[serde(flatten)]
    pub conversation: ConversationView,
    pub items: Vec<HistoryItem>,
}

/// Where an item stands in a participant's history, in the order the viewer
/// reads it: its conversation, and the row the ledger keeps it in, which never
/// changes, so that items stand in the order they were first copied whatever is
/// copied again. A search that stopped inside a long item says how many of its
/// parts it went through (`part`, none where it went through the item whole).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryKey {
    pub conversation: i64,
    pub row: i64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub part: usize,
}

fn is_zero(part: &usize) -> bool {
    *part == 0
}

/// Where a read of a participant's history begins, and which way it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryFrom {
    /// The most recent items, newest first.
    Newest,
    /// The items before this one, newest first.
    Before(HistoryKey),
    /// The items after this one, oldest first.
    After(HistoryKey),
}

/// The part of a long text that is shown: how many characters come before
/// it, the size of the whole text, in bytes, and which copy of its item it is
/// of (as a [`TextPart`]'s `revision`), so that the whole of it read later is
/// known to be of another where the item was copied again changed meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TextCut {
    pub from: usize,
    pub bytes: usize,
    pub revision: i64,
}

/// An item as a read of the history gives it: where it stands, and its text
/// whole, or the part of it that is shown (`cut`).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub key: HistoryKey,
    pub item: HistoryItem,
    pub cut: Option<TextCut>,
}

/// How much of the history a read went through: the conversations it came
/// to, the rows it read, the lookups of parts of a long item's text it made
/// (`parts`), the bytes of text it read of them, the bytes of the items' ids as
/// a page shows them and their times (`fields`), and the steps SQLite's machine
/// took for it all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scanned {
    pub conversations: usize,
    pub rows: usize,
    pub parts: usize,
    pub bytes: usize,
    pub fields: usize,
    pub steps: usize,
}

/// One read of a participant's history: the items it took, across its
/// conversations, in the order it went; where the next read that way goes on
/// (none: there is no more that way); whether the read stopped at the most a
/// read goes through; how much it went through, and what that was charged of
/// the most it may go through (`charged`, in bytes), which what a page reads
/// besides is charged against too.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRead {
    pub participant: i64,
    pub entries: Vec<HistoryRow>,
    pub resume: Option<HistoryKey>,
    pub bounded: bool,
    pub scanned: Scanned,
    pub charged: usize,
}

/// A part of an item's text: from which character, where the next part
/// begins (none: this is its last), and which copy of the item it is of
/// (`revision`: the copy writes the item again changed as the next), so that a
/// reader of every part knows they are of one copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextPart {
    pub text: String,
    pub from: usize,
    pub next: Option<usize>,
    pub revision: i64,
}

/// A message as far as the history page needs it, to say whether a delivery
/// header in a participant's window names one of that participant's: the
/// project, and who it was to and from. Its body is never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageParties {
    pub id: i64,
    pub project_id: i64,
    pub recipient_id: i64,
    pub sender: Option<String>,
}

/// One item a task's windows wrote, and the conversation it is from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptItem {
    pub id: String,
    pub conversation: i64,
    pub role: String,
    pub text: String,
    pub complete: bool,
    pub at: Option<String>,
    /// A compaction's summary, as in [`HistoryItem`]: said only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub compaction: bool,
    /// What its reader said of it besides its words, as in [`HistoryItem`], but the change
    /// an edit made, which a task's transcript never shows.
    #[serde(flatten)]
    pub detail: Option<ItemDetail>,
}

/// What a task's windows wrote: how many items in all, and the last ones asked for.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskTranscript {
    /// Whether the task reached a window: one whose brief reached none has no
    /// transcript of its own. Said only when it did not, which Node never said.
    #[serde(skip_serializing_if = "did")]
    pub reached: bool,
    pub total: usize,
    pub items: Vec<TranscriptItem>,
}

/// Whether what a field says happened: such a field is left unsaid.
fn did(happened: &bool) -> bool {
    *happened
}

/// What the chief ran on before the latest Switch chief: its harness, and the model
/// picked for it where one was (none for a chief from before a chief had one).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SwitchedFrom {
    pub harness: String,
    pub model: Option<String>,
}

/// The project's latest Switch chief: what the chief was switched from, and
/// whether its turn was cut.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastSwitch {
    pub from: SwitchedFrom,
    pub cut: bool,
}

/// A task another task needs, with its state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Need {
    pub number: i64,
    pub state: String,
}

/// A task on the board.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: i64,
    pub project_id: i64,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub state: String,
    pub requester: String,
    pub assignee: Option<String>,
    pub pool: Option<String>,
    pub tier: Option<String>,
    pub purpose: Option<String>,
    /// The assignee, when it is a member's session.
    pub session: Option<String>,
    pub needs: Vec<Need>,
    /// The numbers of the tasks it needs that are not accepted yet.
    pub blocked_by: Vec<i64>,
    pub held_until: Option<String>,
    pub paused_at: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// What ConsensFlow shows of the task instead of telling its chief, which asks no
    /// decision: the words of its newest note that was shown, not typed, while it holds
    /// (until the task moves on). None where none does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aside: Option<String>,
}

/// A message in an inbox.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: i64,
    pub project_id: i64,
    pub recipient: String,
    pub recipient_id: i64,
    pub recipient_role: String,
    pub sender: Option<String>,
    pub kind: String,
    pub task_number: Option<i64>,
    pub reply_to: Option<i64>,
    pub body: String,
    pub state: String,
    pub attempts: i64,
    pub reason: Option<String>,
    /// What the delivery left as proof, as it was stored.
    pub receipt: Value,
    pub questions: Value,
    pub choices: Value,
    pub urgent: bool,
    pub created_at: String,
    pub delivered_at: Option<String>,
}

/// A task with its whole thread, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskThread {
    #[serde(flatten)]
    pub task: TaskView,
    pub messages: Vec<MessageView>,
}

/// A new task, the message that took it to its window (none while it waits
/// on the board), and the tier asked when the task went to the nearest one
/// somebody on the staff holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskCreated {
    pub task: TaskView,
    pub message: Option<MessageView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
}

/// A task that moved, and the message that moves it on (none when it went
/// back to the board). Of a step the chief finished by its word: `withdrawn`
/// is what was still on its way to the chief about it and was taken back (its
/// brief, and any answer or note carried on it); `landing` is its brief, if it
/// was being typed in, which cannot be taken back and may still arrive once.
/// Both are empty, and not serialized, otherwise.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskMoved {
    pub task: TaskView,
    pub message: Option<MessageView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub withdrawn: Vec<MessageView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub landing: Vec<MessageView>,
}

/// A task taken back to the board.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskReleased {
    pub task: TaskView,
}

/// An open task whose tier nobody on the staff holds any more, which went to
/// the nearest one somebody does: the tier it was for, the tier it is for now,
/// and the members of that tier.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskRetiered {
    pub from: String,
    pub to: String,
    pub holders: Vec<String>,
}

/// How the tasks a task needs, or that wait for it, are changed: the list is
/// replaced, or some are added and some removed.
#[derive(Debug, Clone, PartialEq)]
pub enum Links {
    Set(Vec<i64>),
    Change { add: Vec<i64>, remove: Vec<i64> },
}

/// A task whose brief, tier, needs or waiters were changed before a window had
/// it: the task as it is now; the tier asked and the members of the one it went
/// to, when its new tier is one nobody holds and it went to the nearest; and the
/// tasks that wait for it now, when those were changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskEdited {
    pub task: TaskView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holders: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<Vec<i64>>,
}

/// What changes in a task no window has: its brief, its tier, the tasks it
/// needs and the tasks that wait for it, in one step.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TaskEdit {
    pub body: Option<String>,
    pub tier: Option<String>,
    pub needs: Option<Links>,
    pub before: Option<Links>,
}

/// A held task whose time has come.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldTask {
    pub project_id: i64,
    pub number: i64,
    pub assignee_id: Option<i64>,
}

/// A message whose delivery has begun, and the rows that ride in its paste
/// (the ones it carries, still waiting, by id). The ledger never serialises
/// it: the engine writes the paste from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Begun {
    pub message: MessageView,
    pub carried: Vec<MessageView>,
}

/// What a door finds when it asks for the answer to its question: the
/// answer, now claimed for it (boxed: it is the one variant that holds a
/// row); none yet; or the door is shut.
#[derive(Debug, Clone, PartialEq)]
pub enum Claim {
    Answered(Box<MessageView>),
    Waiting,
    Closed,
}

/// The stops asked of a window's task: how many its pauses have asked so
/// far, which is the identity of the last. Two pauses in one millisecond are
/// two stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    pub task_id: i64,
    pub number: i64,
    pub seq: i64,
}

/// One option of a question, as a harness's own question tool offers it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

/// A question with options, as a harness's own question tool asks it: its
/// text, a short header, its options, and whether several may be picked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multiple: bool,
}

/// A task as its card on the board shows it: the task without its brief
/// (the drawer reads that with the task), and the first line of its latest
/// result, none before there is one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCard {
    pub id: i64,
    pub project_id: i64,
    pub number: i64,
    pub title: String,
    pub state: String,
    pub requester: String,
    pub assignee: Option<String>,
    pub pool: Option<String>,
    pub tier: Option<String>,
    pub purpose: Option<String>,
    pub session: Option<String>,
    pub needs: Vec<Need>,
    pub blocked_by: Vec<i64>,
    pub held_until: Option<String>,
    pub paused_at: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub result: Option<String>,
    /// What ConsensFlow shows of the task instead of telling its chief ([`TaskView::aside`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aside: Option<String>,
}

impl TaskCard {
    /// `task`'s card, `result` the first line of its latest result.
    pub fn of(task: TaskView, result: Option<String>) -> Self {
        Self {
            id: task.id,
            project_id: task.project_id,
            number: task.number,
            title: task.title,
            state: task.state,
            requester: task.requester,
            assignee: task.assignee,
            pool: task.pool,
            tier: task.tier,
            purpose: task.purpose,
            session: task.session,
            needs: task.needs,
            blocked_by: task.blocked_by,
            held_until: task.held_until,
            paused_at: task.paused_at,
            deleted_at: task.deleted_at,
            created_at: task.created_at,
            updated_at: task.updated_at,
            result,
            aside: task.aside,
        }
    }
}

/// One participant's lane on the board, with the tasks on it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lane {
    pub participant: ParticipantView,
    pub tasks: Vec<TaskCard>,
}

/// What the human's Resume of a suspended project has to start: the tasks not
/// finished, and the messages waiting for the windows of the project's agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resume {
    pub tasks: i64,
    pub messages: i64,
}

/// The board as the page reads it: the project, the tasks no lane has, each
/// participant's lane, what waits for the human's approval, and what a meeting
/// with the chief holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Board {
    pub project: ProjectView,
    /// Every task no lane has, whatever its state: one waiting for a member,
    /// one paused, called off or failed before any member had it, and one of a
    /// member who left the staff. A task the human deleted is on no list.
    pub open: Vec<TaskCard>,
    pub lanes: Vec<Lane>,
    pub gated: Vec<MessageView>,
    /// What a meeting with the chief holds, in the order it will go: said only
    /// while it holds something (3.0.7 chief-meeting).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<MessageView>,
    /// What waits for the human's Resume, said of a suspended project alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<Resume>,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// A view whose body may have been cut to fit a frame, and says so.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cut<T> {
    #[serde(flatten)]
    pub view: T,
    #[serde(rename = "bodyCut", skip_serializing_if = "is_false")]
    pub body_cut: bool,
}

/// A task with its thread as the page reads it in one frame: bodies cut
/// to fit, and how many of its earliest messages were left out, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskThatFits {
    #[serde(flatten)]
    pub task: Cut<TaskView>,
    pub messages: Vec<Cut<MessageView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages_left_out: Option<usize>,
}

/// The newest of a window's transcript that fit in a frame, in order: how
/// many items there are, and how many came.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestTranscript {
    pub items: Vec<TranscriptItem>,
    pub total: usize,
    pub shown: usize,
}

/// A participant's newest messages that fit in a frame, newest first: how
/// many there are, and how many came.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LatestMessages {
    pub messages: Vec<MessageView>,
    pub total: i64,
    pub shown: usize,
}

/// What waits on the chief, for a chief that takes over: members' questions
/// to it without an answer, results it has not decided on, its own
/// unfinished tasks, and the tasks it gave that are held (out of quota, an
/// account unavailable, a model moved), each with what its line says of the
/// hold (`aside`), or paused with their member gone from the staff, saying so.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChiefOpenWork {
    pub questions: Vec<MessageView>,
    pub results: Vec<TaskView>,
    pub own: Vec<TaskView>,
    /// Absent where none is held, as a ledger before RS-25 wrote it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<TaskView>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_participant_with_its_fields_in_the_node_ledger_s_order() {
        let view = ParticipantView {
            id: 2,
            project_id: 1,
            handle: "zeus-amber-pine".into(),
            role: "worker".into(),
            agent: Some("zeus".into()),
            harness: Some("pi".into()),
            model: None,
            effort: None,
            model_name: None,
            former_handles: Vec::new(),
            name_apart: false,
            needs_edit: false,
            launched_model: None,
            gone_model: None,
            ran_on: None,
            designer: false,
            created_at: "2026-09-19T10:00:01.000Z".into(),
            left_at: None,
            tier: Some("standard".into()),
            roles: vec!["worker".into()],
            out_until: None,
            out_since: None,
            member_id: Some(1),
            member: Some("zeus".into()),
            session: Some("amber-pine".into()),
        };
        assert_eq!(
            serde_json::to_string(&view).unwrap(),
            r#"{"id":2,"projectId":1,"handle":"zeus-amber-pine","role":"worker","agent":"zeus","harness":"pi","designer":false,"createdAt":"2026-09-19T10:00:01.000Z","leftAt":null,"tier":"standard","roles":["worker"],"outUntil":null,"outSince":null,"memberId":1,"member":"zeus","session":"amber-pine"}"#
        );
    }

    /// A chief's model is said after its harness, and only by a chief that has one; the
    /// model its last launch asked for is not said: a member's view reads as Node's did
    /// (CHF-04).
    #[test]
    fn writes_a_chiefs_model_after_its_harness() {
        let chief = ParticipantView {
            id: 2,
            project_id: 1,
            handle: "chief".into(),
            role: "chief".into(),
            agent: None,
            harness: Some("pi".into()),
            model: Some("openrouter/anthropic/claude-fable-5".into()),
            effort: None,
            model_name: None,
            former_handles: Vec::new(),
            name_apart: false,
            needs_edit: false,
            launched_model: Some("openai/gpt-6-luna".into()),
            gone_model: None,
            ran_on: None,
            designer: false,
            created_at: "2026-09-19T10:00:01.000Z".into(),
            left_at: None,
            tier: None,
            roles: Vec::new(),
            out_until: None,
            out_since: None,
            member_id: None,
            member: None,
            session: None,
        };
        assert_eq!(
            serde_json::to_string(&chief).unwrap(),
            r#"{"id":2,"projectId":1,"handle":"chief","role":"chief","agent":null,"harness":"pi","model":"openrouter/anthropic/claude-fable-5","designer":false,"createdAt":"2026-09-19T10:00:01.000Z","leftAt":null,"tier":null,"roles":[],"outUntil":null,"outSince":null,"memberId":null,"member":null,"session":null}"#
        );
    }

    #[test]
    fn writes_a_candidate_as_its_member_with_had_it_last() {
        let candidate = Candidate {
            member: MemberView {
                id: 3,
                handle: "zeus".into(),
                agent: Some("zeus".into()),
                harness: Some("pi".into()),
                tier: Some("standard".into()),
                roles: vec!["worker".into()],
                taken: 2,
                sessions: 1,
                out_until: None,
                needs_edit: false,
            },
            had_it: true,
        };
        assert_eq!(
            serde_json::to_string(&candidate).unwrap(),
            r#"{"id":3,"handle":"zeus","agent":"zeus","harness":"pi","tier":"standard","roles":["worker"],"taken":2,"sessions":1,"outUntil":null,"hadIt":true}"#
        );
    }

    #[test]
    fn writes_a_chief_conversation_as_its_conversation_with_its_items_last() {
        let conversation = ChiefConversation {
            conversation: ConversationView {
                id: 4,
                participant_id: 2,
                harness: "codex".into(),
                native_session: Some("s-1".into()),
                started_at: "2026-09-19T10:00:01.000Z".into(),
                ended_at: Some("2026-09-19T10:00:02.000Z".into()),
            },
            items: vec![HistoryItem {
                id: "i-1".into(),
                role: "user".into(),
                text: "hello".into(),
                complete: true,
                at: None,
                compaction: false,
                detail: None,
            }],
        };
        assert_eq!(
            serde_json::to_string(&conversation).unwrap(),
            r#"{"id":4,"participantId":2,"harness":"codex","nativeSession":"s-1","startedAt":"2026-09-19T10:00:01.000Z","endedAt":"2026-09-19T10:00:02.000Z","items":[{"id":"i-1","role":"user","text":"hello","complete":true,"at":null}]}"#
        );
    }

    #[test]
    fn says_a_compaction_s_summary_only_where_an_item_is_one() {
        let item = |compaction| HistoryItem {
            id: "s-1".into(),
            role: "user".into(),
            text: "This session is being continued".into(),
            complete: true,
            at: None,
            compaction,
            detail: None,
        };
        assert_eq!(
            serde_json::to_string(&item(true)).unwrap(),
            r#"{"id":"s-1","role":"user","text":"This session is being continued","complete":true,"at":null,"compaction":true}"#
        );
        assert!(!serde_json::to_string(&item(false))
            .unwrap()
            .contains("compaction"));
    }

    #[test]
    fn a_place_says_the_part_a_search_stopped_at_inside_an_item_and_no_part_elsewhere() {
        let inside = HistoryKey {
            conversation: 4,
            row: 90,
            part: 3,
        };
        let said = serde_json::to_string(&inside).unwrap();
        assert_eq!(said, r#"{"conversation":4,"row":90,"part":3}"#);
        assert_eq!(serde_json::from_str::<HistoryKey>(&said).unwrap(), inside);
        let whole = HistoryKey { part: 0, ..inside };
        assert_eq!(
            serde_json::to_string(&whole).unwrap(),
            r#"{"conversation":4,"row":90}"#
        );
        assert_eq!(
            serde_json::from_str::<HistoryKey>(r#"{"conversation":4,"row":90}"#).unwrap(),
            whole
        );
    }
}
