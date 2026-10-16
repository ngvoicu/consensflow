//! What the app says of specs for every project that does not say otherwise,
//! which Settings has a place for: whether they are on, and the folder and the
//! Specmint variant a project uses when it names none. Kept in the agents file
//! beside the human's choice about the roster ([`cf_catalog::Roster::app_specs`]),
//! and the human's alone, as a project's own word is: the page is the app's, and
//! neither the agents' API nor the screens' route of the roster's choice says or
//! takes them. A change applies at a chief's next launch, which reads them as
//! they are then.
//!
//! The page is answered what it draws from, whatever it asked: the raw word of
//! the app (what Settings shows and edits), what a project that names none then
//! uses (the documented folder and variant where the app names none, which no
//! page types again), and the Specmint this build bundles.

use cf_proto::specs::{AppSpecs, Bundled};
use serde_json::json;

use super::agents::saved;
use super::body::{Body, Fields, Said};
use super::Page;

/// `specs.get`.
pub(super) async fn get(page: &Page) -> Result<Fields, Said> {
    let app = saved(&page.env)?.roster().app_specs()?;
    answered(&app, &cf_harness::bundled_specmint())
}

/// `specs.set`: each part the page gives is kept, `on` as a switch, `folder`
/// and `variant` as text or `null` to name none again; a part it leaves out is
/// as it was. Each is checked as a project's own is, and a refusal keeps none.
pub(super) async fn set(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let app = saved(&page.env)?.roster().set_app_specs(body.json())?;
    answered(&app, &cf_harness::bundled_specmint())
}

/// What both answer, `{app, applied, specmint}`, of `app` and the Specmint
/// `bundled`. What the bundled files do not say of themselves is unknown in
/// `specmint`, and fails nothing: the human's settings come back whatever they
/// say, and the dialogs that read them open.
pub(super) fn answered(app: &AppSpecs, bundled: &Bundled) -> Result<Fields, Said> {
    let mut fields = Fields::new();
    fields.insert("app".to_owned(), serde_json::to_value(app)?);
    fields.insert(
        "applied".to_owned(),
        json!({
            "folder": app.folder_or_default(),
            "variant": app.variant_or_default(),
        }),
    );
    fields.insert("specmint".to_owned(), serde_json::to_value(bundled)?);
    Ok(fields)
}
