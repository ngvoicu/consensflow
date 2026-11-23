//! The harness diagnostics' route: what is known of each harness's CLI here, as the harness
//! admin ([`HarnessAdmin`](cf_harness::admin::HarnessAdmin)) says it. The page is read-only:
//! it installs nothing, and says the command that updates a harness the way it was installed.
//! The screen only checks that the harness asked about is one of the five, before the machine
//! is asked anything.

use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::body::property;
use super::Screens;
use crate::api::answer::Answer;

/// What both routes say of a harness that is none of the five.
const UNKNOWN: &str = "Unknown harness";

/// `['claude', 'codex', 'pi', 'devin'].includes(id)`, and `muse` since 3.0.6: a text that
/// names one of them, and nothing else does, whatever it is.
fn known(id: Option<&Value>) -> Result<Harness, String> {
    id.and_then(Value::as_str)
        .and_then(Harness::from_name)
        .ok_or_else(|| UNKNOWN.to_owned())
}

/// A row as the page reads it: its fields and its `stamp`, the number its look took when it began.
fn stamped(row: &impl serde::Serialize, stamp: u64) -> Value {
    let mut row = serde_json::to_value(row).unwrap_or(Value::Null);
    if let Some(fields) = row.as_object_mut() {
        fields.insert("stamp".to_owned(), json!(stamp));
    }
    row
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
        let observed = self.admin.observe(id.map(Harness::as_str), refresh).await?;
        let harnesses: Vec<Value> = observed
            .iter()
            .map(|(stamp, row)| stamped(row.as_ref(), *stamp))
            .collect();
        Ok(Answer::ok(json!({ "harnesses": harnesses })))
    }
}
