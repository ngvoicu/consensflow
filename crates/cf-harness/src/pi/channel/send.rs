//! One message sent to the extension's inbox, as exact text, without a result
//! envelope. The inbox record is strictly bounded `{id, type, launchId,
//! session, text, expiresAt}`, with a unique `m-<hex>` id, the launch's
//! immutable id, the target's conversation and one absolute expiry. Native
//! admission is gated by the pane's claim immediately before the inbox rename,
//! the hand-over: a failed claim, or any failure before the rename, is known to
//! have written nothing ([`before`]). From the rename on, Pi may have taken the
//! message, so a missing acknowledgement or any failure is uncertain and Pi's
//! own record decides ([`after`]); the message is never retried automatically.

use std::path::Path;
use std::time::Duration;

use cf_base::file::{make_folder, read_file, rename, rm_force, write_file, FileError, Mkdir};
use cf_base::json::from_slice_lossy;
use cf_base::{js, path};
use serde_json::{json, Value};

use super::hex;
use crate::contract::{Pane, PaneHost};
use crate::seams::{Entropy, Time};
use crate::shared::admission::Sent;
use crate::shared::pane::claim;

/// How often the channel looks for the extension's verdict.
const ACK_POLL_MS: i64 = 10;

/// How long past a record's expiry the channel still looks for the
/// extension's verdict. The extension gives one at the expiry, from a timer
/// in Pi's process and then three file operations, so on a busy machine it
/// lands late: 30 ms missed it on CI. A verdict seen late is still Pi's own;
/// one missed leaves the message uncertain, for its record to decide.
const ACK_GRACE_MS: i64 = 1000;

/// The largest whole number JavaScript holds exactly.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Where a message goes and what it is for.
pub struct Target<'a> {
    pub launch_id: &'a str,
    pub inbox: &'a str,
    pub ack: &'a str,
    /// How long the extension has to answer, from the moment the record is
    /// written.
    pub ack_timeout_ms: u64,
    /// The conversation the message is for.
    pub session: &'a str,
    /// The pane the send is claimed through.
    pub pane: &'a Pane,
    pub host: &'a dyn PaneHost,
}

/// What a send answered (the object `send` returned): what Pi's window did
/// with the message, as far as the channel can tell.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub ok: bool,
    /// Whether Pi took the message: its own word, or a refusal before the
    /// hand-over said so; none where it may have.
    pub admitted: Option<bool>,
    pub error: Option<&'static str>,
    /// `bytesWritten: 0`: nothing reached Pi.
    pub zero_bytes: bool,
    pub cause: Option<String>,
    /// The extension's own acknowledgement, as it wrote it.
    pub ack: Option<Value>,
}

impl Answer {
    /// Refused before anything was written.
    fn refused(error: &'static str) -> Self {
        Self {
            ok: false,
            admitted: Some(false),
            error: Some(error),
            zero_bytes: false,
            cause: None,
            ack: None,
        }
    }

    /// Refused before the hand-over, with a record the channel wrote or was to
    /// write: nothing reached Pi.
    fn zero_bytes(error: &'static str, cause: Option<String>) -> Self {
        Self {
            zero_bytes: true,
            cause,
            ..Self::refused(error)
        }
    }

    /// A failure before the inbox rename (`zeroByteTransport`).
    fn transport(cause: String) -> Self {
        Self::zero_bytes("transport", Some(cause))
    }

    /// A claim the host refused, in the host's own words (`zeroByteClaimRefusal`).
    fn claim_refused(claimed: &Value) -> Self {
        let word = |name: &str| {
            claimed
                .get(name)
                .filter(|value| !value.is_null())
                .map(|value| js::text(Some(value)).into_owned())
        };
        let cause = word("cause")
            .or_else(|| word("error"))
            .unwrap_or_else(|| "claim-refused".to_owned());
        Self::zero_bytes("failed-with-zero-bytes", Some(cause))
    }

    /// Pi may or may not have taken the message.
    fn uncertain(cause: Option<String>, ack: Option<Value>) -> Self {
        Self {
            ok: false,
            admitted: None,
            error: Some("uncertain"),
            zero_bytes: false,
            cause,
            ack,
        }
    }

    /// What the extension's acknowledgement says of the message
    /// (`readAckResult`): none came, it says nothing yet (`admitted: null`),
    /// it refused the message before it sent it, it took it, or it says
    /// something no verdict says.
    fn from_ack(ack: Option<Value>) -> Self {
        let Some(ack) = ack else {
            return Self::uncertain(Some("admission-unknown".to_owned()), None);
        };
        match ack.get("admitted") {
            Some(Value::Null) => Self::uncertain(Some("admission-unknown".to_owned()), Some(ack)),
            Some(Value::Bool(false)) => Self {
                ok: false,
                admitted: Some(false),
                error: Some("failed-with-zero-bytes"),
                zero_bytes: ack.get("bytesWritten").and_then(Value::as_f64) == Some(0.0),
                cause: None,
                ack: Some(ack),
            },
            Some(Value::Bool(true)) => Self {
                ok: true,
                admitted: Some(true),
                error: None,
                zero_bytes: false,
                cause: None,
                ack: Some(ack),
            },
            _ => Self::uncertain(Some("invalid-admission".to_owned()), Some(ack)),
        }
    }

    /// The answer as an adapter reads a send's (`admission`).
    pub(crate) fn reading(&self) -> Sent {
        Sent {
            ok: self.ok,
            refused: self.admitted == Some(false),
            cause: self.cause.clone(),
            error: self.error.map(str::to_owned),
        }
    }
}

/// Why a send did not reach the hand-over: with an answer of its own, or
/// with a failure that JavaScript threw.
enum Failed {
    Answered(Answer),
    Threw(String),
}

/// A message that reached the hand-over: its record is in a temporary beside
/// the inbox, and its pane is claimed.
struct Handover {
    id: String,
    expires_at: i64,
    inbox_file: String,
    ack_file: String,
    temporary: String,
}

/// Sends `text` to the window `target` names, and waits for the extension's
/// verdict: what became of it, or why it could not even be sent (the system
/// would not give the id its randomness).
pub async fn send(
    time: &dyn Time,
    entropy: &dyn Entropy,
    target: &Target<'_>,
    text: &str,
) -> Result<Answer, String> {
    match before(time, entropy, target, text).await {
        Ok(handover) => Ok(after(time, handover).await),
        Err(Failed::Answered(answer)) => Ok(answer),
        Err(Failed::Threw(thrown)) => Err(thrown),
    }
}

/// Everything up to the inbox rename: the record written to a temporary beside
/// the inbox, and the pane claimed. A failure here is known to have reached
/// nothing, and the temporary is gone.
async fn before(
    time: &dyn Time,
    entropy: &dyn Entropy,
    target: &Target<'_>,
    text: &str,
) -> Result<Handover, Failed> {
    if text.is_empty() || target.session.is_empty() {
        return Err(Failed::Answered(Answer::refused("invalid-record")));
    }
    let mut drawn = [0; 16];
    entropy.fill(&mut drawn).map_err(Failed::Threw)?;
    let id = format!("m-{}", hex(&drawn));
    let timeout = i64::try_from(target.ack_timeout_ms).unwrap_or(i64::MAX);
    let expires_at = time.wall_ms().saturating_add(timeout);
    if expires_at <= time.wall_ms() {
        return Err(Failed::Answered(Answer::zero_bytes("expired", None)));
    }
    let record = json!({
        "id": id,
        "type": "message",
        "launchId": target.launch_id,
        "session": target.session,
        "text": text,
        "expiresAt": expires_at,
    });
    let inbox_file = path::join(&[target.inbox, &format!("{id}.json")]);
    let ack_file = path::join(&[target.ack, &format!("{id}.json")]);
    let temporary = format!("{inbox_file}.tmp");
    let before_the_rename = |cause: String| {
        // A temporary that cannot be removed is no more than it was.
        let _ = rm_force(Path::new(&temporary));
        Failed::Answered(Answer::transport(cause))
    };
    if let Err(failed) = write_record(target, &record, &ack_file, &temporary) {
        return Err(before_the_rename(failed.to_string()));
    }
    pane_named(target.pane).map_err(before_the_rename)?;
    let claimed = claim(target.host, target.pane).await;
    if claimed.get("ok") != Some(&Value::Bool(true)) {
        return Err(match rm_force(Path::new(&temporary)) {
            Ok(()) => Failed::Answered(Answer::claim_refused(&claimed)),
            Err(failed) => before_the_rename(failed.to_string()),
        });
    }
    Ok(Handover {
        id,
        expires_at,
        inbox_file,
        ack_file,
        temporary,
    })
}

/// The folders the record and the acknowledgement go in, made if they are
/// not there, and the record written to its temporary (`publishRaw`).
fn write_record(
    target: &Target<'_>,
    record: &Value,
    ack_file: &str,
    temporary: &str,
) -> Result<(), FileError> {
    make_folder(Path::new(target.inbox), 0o777, Mkdir::Promise)?;
    // The folder of the file `join` made of the acknowledgements' folder.
    if let Some(folder) = Path::new(ack_file).parent() {
        make_folder(folder, 0o777, Mkdir::Promise)?;
    }
    let text = format!("{}\n", js::stringify(record));
    write_file(Path::new(temporary), text.as_bytes(), 0o666)
}

/// The pane a claim can name: a generation is a whole number from 1, and one
/// JavaScript holds exactly.
fn pane_named(pane: &Pane) -> Result<(), String> {
    if (1..=MAX_SAFE_INTEGER).contains(&pane.generation) {
        Ok(())
    } else {
        Err("native delivery needs pane {id, generation}".to_owned())
    }
}

/// From the inbox rename on: Pi may be taking the message, so no failure is
/// an error, and what is not Pi's own word is uncertain. A message nobody
/// acknowledged is taken out of the inbox; one that failed stays there, as
/// the extension refuses it once it expires.
async fn after(time: &dyn Time, handover: Handover) -> Answer {
    let Handover {
        id,
        expires_at,
        inbox_file,
        ack_file,
        temporary,
    } = handover;
    let acknowledged = async {
        rename(Path::new(&temporary), Path::new(&inbox_file))?;
        let ack = ack_for(time, &ack_file, &id, expires_at).await?;
        if ack.is_none() {
            rm_force(Path::new(&inbox_file))?;
        }
        Ok::<_, FileError>(ack)
    };
    match acknowledged.await {
        Ok(ack) => Answer::from_ack(ack),
        Err(failed) => {
            // A temporary that cannot be removed is no more than it was.
            let _ = rm_force(Path::new(&temporary));
            Answer::uncertain(Some(failed.to_string()), None)
        }
    }
}

/// The extension's acknowledgement of the message `id`, which it writes to
/// `file`, looked for until a grace past the record's expiry, or none. A file
/// that is not there yet, or is no JSON yet, or another message's, is not it;
/// one the system will not read is a failure.
///
/// The last millisecond is slept out, not read through: a loop that does not
/// wait reads the file again and again until the clock moves.
async fn ack_for(
    time: &dyn Time,
    file: &str,
    id: &str,
    expires_at: i64,
) -> Result<Option<Value>, FileError> {
    let deadline = expires_at.saturating_add(ACK_GRACE_MS);
    while time.wall_ms() <= deadline {
        match read_file(Path::new(file)) {
            Ok(bytes) => {
                if let Ok(ack) = from_slice_lossy(&bytes) {
                    if ack.get("id").and_then(Value::as_str) == Some(id) {
                        return Ok(Some(ack));
                    }
                }
            }
            Err(failed) if failed.code() == "ENOENT" => {}
            Err(failed) => return Err(failed),
        }
        let remaining = deadline - time.wall_ms();
        if remaining >= 0 {
            let wait = remaining.clamp(1, ACK_POLL_MS);
            time.sleep(Duration::from_millis(wait.unsigned_abs())).await;
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
