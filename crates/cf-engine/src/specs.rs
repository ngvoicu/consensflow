//! What a chief's launch is given of its project's specs. A project's specs are
//! on when it says so, or says nothing and the human turned them on for every
//! project, and they are kept in the folder and planned with the variant the
//! project names, else the ones the human named for every project, else the
//! documented ones (`cf_proto::specs`); only its chief is given them, and only
//! at a launch, which is when a change of the setting applies (a Resume, a
//! switch).
//! The harness says where its window finds the Specmint skill of the project's
//! variant ([`Adapter::specmint`]); that place is named in the chief's role
//! text ([`ProjectSpecs`]) and given the adapter, which has the skill there,
//! written or its own, when it prepares the window ([`Launch::specs`]). The
//! place is the harness's and the variant's, never a launch's, so the role text
//! of a chief whose project's specs did not change is the same at every launch.
//! A project with specs off, and every member, are asked about nothing and given
//! nothing: their launch is as it was.

use cf_harness::contract::{Adapter, Skill, Specs};
use cf_ledger::{ParticipantView, ProjectView};
use cf_proto::specs::{AppSpecs, SpecsSetting};

use crate::dispatcher::Dispatcher;
use crate::roles::ProjectSpecs;
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
            skill: &self.skill,
        }
    }
}

impl Dispatcher {
    /// The text `participant`'s window starts with in `project`, with the specs
    /// its chief is given. Asked once for a launch, where the skill is named in
    /// the text and is the adapter's to write.
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
        Ok(RoleText {
            specs,
            instructions,
        })
    }

    /// The specs `participant`'s launch is given in `project`: none for any
    /// member, and none for a chief whose project has them off. What the app
    /// says (whether, the folder and the variant) is asked of the roster unless
    /// the project says off, which nothing the app says could change.
    pub(crate) fn chief_specs(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        adapter: &dyn Adapter,
    ) -> Result<Option<ChiefSpecs>, EngineError> {
        if participant.role != "chief" {
            return Ok(None);
        }
        let says_off = project.specs.as_ref().and_then(|specs| specs.on) == Some(false);
        let app = if says_off {
            AppSpecs::default()
        } else {
            self.seams.roster.app_specs()
        };
        let Some(setting) = project.specs_setting(&app) else {
            return Ok(None);
        };
        let skill = adapter
            .specmint(setting.variant)
            .map_err(|cause| EngineError::said("launch-failed", cause))?;
        Ok(Some(ChiefSpecs { setting, skill }))
    }
}
