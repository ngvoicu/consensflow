//! What a chief's launch is given of its project's specs. A project's specs are
//! on when it says so, or says nothing and the human turned them on for every
//! project (`cf_proto::specs`); only its chief is given them, and only at a
//! launch, which is when a change of the setting applies (a Resume, a switch).
//! The harness says where its window finds the Specmint skill of the project's
//! variant ([`Adapter::specmint`]); that place is named in the chief's role
//! text ([`ProjectSpecs`]) and given the adapter, which has the skill there,
//! written or its own, when it prepares the window ([`Launch::specs`]). A
//! project with specs off, and every member, are asked about nothing and given
//! nothing: their launch is as it was.

use cf_harness::contract::{Adapter, LaunchId, Skill, Specs};
use cf_ledger::{ParticipantView, ProjectView};
use cf_proto::specs::SpecsSetting;

use crate::dispatcher::Dispatcher;
use crate::roles::ProjectSpecs;
use crate::seams::EngineError;

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
    /// The specs `participant`'s launch is given in `project`: none for any
    /// member, and none for a chief whose project has them off. The app's
    /// default is asked only where the project says nothing of its own.
    pub(crate) fn chief_specs(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        adapter: &dyn Adapter,
        launch: &LaunchId,
    ) -> Result<Option<ChiefSpecs>, EngineError> {
        if participant.role != "chief" {
            return Ok(None);
        }
        let says = project.specs.as_ref().and_then(|specs| specs.on);
        let app_default = says.is_none() && self.seams.roster.specs_by_default();
        let Some(setting) = project.specs_setting(app_default) else {
            return Ok(None);
        };
        let skill = adapter
            .specmint(setting.variant, launch)
            .map_err(|cause| EngineError::said("launch-failed", cause))?;
        Ok(Some(ChiefSpecs { setting, skill }))
    }
}
