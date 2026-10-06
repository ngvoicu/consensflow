//! The harness diagnostics' routes (`agents-server.js:109-127`): what is known
//! of each harness's CLI here, and its update. Both are the harness admin's
//! ([`HarnessAdmin`](cf_harness::admin::HarnessAdmin)): the screen only checks
//! that the harness asked about is one of the five, before the machine is
//! asked anything, and says what the admin answered, or why it would not (a
//! harness that is not installed is asked to update: `Claude is not installed`).

use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::body::property;
use super::Screens;
use crate::api::answer::Answer;

/// What both routes say of a harness that is none of the five.
const UNKNOWN: &str = "Unknown harness";

/// `['claude', 'codex', 'opencode', 'pi', 'devin'].includes(id)`: a text that
/// names one of them, and nothing else does, whatever it is.
fn known(id: Option<&Value>) -> Result<Harness, String> {
    id.and_then(Value::as_str)
        .and_then(Harness::from_name)
        .ok_or_else(|| UNKNOWN.to_owned())
}

impl Screens {
    /// `POST /api/harnesses/check`: every harness's row, or the one `id` names;
    /// a row kept less than five minutes is answered as it was, unless `refresh`
    /// is `true` itself.
    pub(super) async fn check_harnesses(&self, body: &Value) -> Result<Answer, String> {
        // An `id` the body has, `null` among them, must name a harness.
        let id = property(body, "id")?
            .map(|id| known(Some(id)))
            .transpose()?;
        let refresh = property(body, "refresh")? == Some(&Value::Bool(true));
        let harnesses = self.admin.check(id.map(Harness::as_str), refresh).await?;
        Ok(Answer::ok(json!({ "harnesses": harnesses })))
    }

    /// `POST /api/harnesses/update`: the harness `id` names, brought to its latest
    /// release the way it was installed, and what became of it.
    pub(super) async fn update_harness(&self, body: &Value) -> Result<Answer, String> {
        let id = known(property(body, "id")?)?;
        let result = self.admin.update(id.as_str()).await?;
        Ok(Answer::ok(json!({ "result": result })))
    }
}
