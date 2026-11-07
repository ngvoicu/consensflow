//! What a chief's launch is given of its project's specs. A project's specs are
//! on when it says so, or says nothing and the human turned them on for every
//! project, and they are kept in the folder and planned with the variant the
//! project names, else the ones the human named for every project, else the
//! documented ones (`cf_proto::specs`); only its chief is given them. They are in the
//! text of a launch (a Resume, a switch), and a chief whose window is open when the
//! setting changes is told in a note at its next rest (`role_news`).
//! The harness says where its window finds the Specmint skill of the project's
//! variant and folder ([`Adapter::specmint`]); that place is named in the chief's role
//! text ([`ProjectSpecs`]) and given the adapter, which has the skill there,
//! written or its own, when it prepares the window ([`Launch::specs`]). The
//! place is the harness's, the variant's and the folder's, never a launch's, so the
//! role text of a chief whose project's specs did not change is the same at every
//! launch. The folder is in it because the skill's own files name `.specs`: a
//! project's folder wins only if the chief is given a copy that names it (T-662).
//! A project with specs off, and every member, are asked about nothing and given
//! nothing: their launch is as it was.

use cf_harness::contract::{Adapter, Skill, Specs};
use cf_ledger::{ParticipantView, ProjectView};
use cf_proto::specs::{AppSpecs, SpecsSetting};

use crate::dispatcher::Dispatcher;
use crate::roles::{with_staff_rules, ProjectSpecs};
use crate::seams::EngineError;

/// The text a launch's window starts with, and the specs it was written for:
/// a chief whose project has specs on has the section of them in its text, and
/// its launch is given the skill the section names.
pub(crate) struct RoleText {
    pub(crate) specs: Option<ChiefSpecs>,
    pub(crate) instructions: String,
}

/// A chief's specs: what the project's setting says, and where the skill is.
pub(crate) struct ChiefSpecs {
    setting: SpecsSetting,
    skill: Skill,
}

impl ChiefSpecs {
    /// What the project says of them.
    pub(crate) fn setting(&self) -> &SpecsSetting {
        &self.setting
    }

    /// What the chief's text says of them.
    pub(crate) fn text(&self) -> ProjectSpecs<'_> {
        ProjectSpecs {
            variant: self.setting.variant,
            folder: &self.setting.folder,
            skill: self.skill.path(),
        }
    }

    /// What the adapter is given of them.
    pub(crate) fn launch(&self) -> Specs<'_> {
        Specs {
            variant: self.setting.variant,
            folder: &self.setting.folder,
            skill: &self.skill,
        }
    }
}

impl Dispatcher {
    /// The text `participant`'s window starts with in `project`, with the specs
    /// its chief is given, and the project's rules for its staff after it. Asked
    /// once for a launch, where the skill is named in the text and is the
    /// adapter's to write; every launch, resume and replacement of a role, on
    /// every harness, has its text from here.
    pub(crate) fn role_text(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        adapter: &dyn Adapter,
    ) -> Result<RoleText, EngineError> {
        let specs = self.chief_specs(project, participant, adapter)?;
        let told = specs.as_ref().map(ChiefSpecs::text);
        let instructions = self
            .seams
            .roles
            .instructions(participant, project, told.as_ref())
            .map_err(|cause| EngineError::said("no-role-text", cause))?;
        let instructions = with_staff_rules(
            instructions,
            &participant.role,
            project.staff_rules.as_deref(),
        );
        Ok(RoleText {
            specs,
            instructions,
        })
    }

    /// What `project` says of specs for `participant` now: none for any member, and
    /// none for a chief whose project has them off. What the app says (whether, the
    /// folder and the variant) is read from the app's settings unless the project says off,
    /// which nothing the app says could change. A running chief is told when this
    /// changes under it ([`Dispatcher::tell_role_news`]).
    pub(crate) fn specs_setting(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> Option<SpecsSetting> {
        if participant.role != "chief" {
            return None;
        }
        let says_off = project.specs.as_ref().and_then(|specs| specs.on) == Some(false);
        let app = if says_off {
            AppSpecs::default()
        } else {
            self.seams.app_settings.app_specs()
        };
        project.specs_setting(&app)
    }

    /// The specs `participant`'s launch is given in `project`: none for any
    /// member, and none for a chief whose project has them off. What the app
    /// says (whether, the folder and the variant) is read from its settings unless
    /// the project says off, which nothing the app says could change.
    pub(crate) fn chief_specs(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        adapter: &dyn Adapter,
    ) -> Result<Option<ChiefSpecs>, EngineError> {
        let Some(setting) = self.specs_setting(project, participant) else {
            return Ok(None);
        };
        let skill = adapter
            .specmint(setting.variant, &setting.folder)
            .map_err(|cause| EngineError::said("launch-failed", cause))?;
        Ok(Some(ChiefSpecs { setting, skill }))
    }
}
