//! Find a task's answers without counting a reply to a role replacement.

use cf_base::js;
use cf_harness::records::{Item, Role};

use super::{Answers, Run};
use crate::delivery_text::header_id;

/// Whether `item` holds words of the member's: some text, and not the harness's own (a
/// note it marks as its progress, `commentary`, or words it wrote in the member's place,
/// `synthetic`).
fn has_words(item: &Item) -> bool {
    !item.commentary && !item.synthetic && !js::trim(&item.text).is_empty()
}

/// A final run, or where it has no words of the member's, the last item with some (`said`),
/// which are its turn's last words.
fn with_words<'a>(run: &[&'a Item], said: Option<&'a Item>) -> Run<'a> {
    match said {
        Some(said) if !run.iter().any(|item| has_words(item)) => Run {
            items: vec![said],
            last_words: true,
        },
        _ => Run {
            items: run.to_vec(),
            last_words: false,
        },
    }
}

/// The answer to the task in `items`, a member's record after its latest message: what
/// is of a turn that answers a replacement of the role is not, and none where nothing
/// else answers. Where a message to the member came after its last words, what answers
/// that is still to come (no `last`), and the last complete answer is kept all the
/// same, for Finish to take. A user's message that says a role begins a turn whose words are the
/// window's word of its role and none of its task's, and the next user's message ends
/// it; it ends no answer that came before it, and a tool's output in it ends none
/// either. `replacements` are the ids of the replacements the ledger knows were written
/// to the member; a message is one by the id of its header, and not by any words. The
/// summary a compaction writes in the user's place is no message to the member at all,
/// and changes nothing of this: Claude Code compacts a conversation that is at rest,
/// after the answer (Astra's review of round 1). What the harness wrote in the
/// conversation itself (a hook's context, a notification it wrote to its record alone)
/// is no message to the member and no answer.
pub(crate) fn answers_to_the_task<'a>(
    items: &'a [Item],
    replacements: &[i64],
) -> Option<Answers<'a>> {
    let mut of_the_role = false;
    let mut last = None;
    let mut final_run = Vec::new();
    let mut complete_run = None;
    // The last item with words of the member's since its latest message, for a final run
    // with none.
    let mut said = None;
    let mut first = FirstRun::default();
    for item in items.iter().filter(|item| !item.compaction) {
        match item.role {
            Role::User => {
                of_the_role =
                    header_id(&item.text).is_some_and(|message| replacements.contains(&message));
                // A message to the member (its own, or its harness's notification) is
                // answered by what follows it, if anything does.
                if !of_the_role {
                    final_run.clear();
                    last = None;
                }
                // Nor are the words before it any of that answer's.
                said = said.filter(|_| of_the_role);
            }
            Role::Tool if !of_the_role => final_run.clear(),
            Role::Assistant if !of_the_role => {
                last = Some(item);
                final_run.push(item);
                if has_words(item) {
                    said = Some(item);
                }
                if item.complete {
                    complete_run = Some(with_words(&final_run, said));
                }
            }
            _ => {}
        }
        first.see(item, of_the_role);
    }
    (last.is_some() || complete_run.is_some()).then_some(Answers {
        last,
        final_run: with_words(&final_run, said),
        complete_run,
        reply: first.reply(),
        after_the_last_tool: final_run,
    })
}

/// What a member wrote before its first tool call, as the walk over its record meets it
/// (`Answers::reply`): the assistant's items from its latest message to its first tool call,
/// and whether a tool call came. A message to the member begins it again, a replacement of the
/// role (`of_the_role`) none; the harness's own words are none of it.
#[derive(Default)]
struct FirstRun<'a> {
    run: Vec<&'a Item>,
    over: bool,
}

impl<'a> FirstRun<'a> {
    /// The next item of the record, `of_the_role` whether it is of a turn on a replacement.
    fn see(&mut self, item: &'a Item, of_the_role: bool) {
        if of_the_role {
            return;
        }
        match item.role {
            Role::User => *self = Self::default(),
            Role::Tool => self.over = true,
            Role::Assistant if !self.over && !item.synthetic => self.run.push(item),
            _ => {}
        }
    }

    /// Its words, where a tool call came after them: with none, the one run is the turn's
    /// final answer, and it is no reply apart from it.
    fn reply(self) -> Vec<&'a Item> {
        if self.over {
            self.run
        } else {
            Vec::new()
        }
    }
}
