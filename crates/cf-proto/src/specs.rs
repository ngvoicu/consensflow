//! Specs, as a project and the app say them. A project's chief may plan big
//! work as specs (Specmint's: a folder of phased plans) and carry each spec's
//! phases onto the board as tasks. Whether it does is the human's choice, off
//! unless they turn it on: for every project at once (what the agents file
//! keeps, `Preferences::specs`), and for one project on its own, which says
//! its own word on whether, the folder it keeps its specs in and the one
//! Specmint variant it plans with ([`SpecsView`]). [`ProjectView::specs_setting`]
//! reads the two together.
//!
//! [`ProjectView::specs_setting`]: crate::ledger::ProjectView::specs_setting

use serde::Serialize;

/// The folder a project keeps its specs in unless it names another, relative to
/// the project's own: where Specmint says `.specs`.
pub const DEFAULT_FOLDER: &str = ".specs";

/// One of the four Specmint variants. Never more than one is given to a chief:
/// they trigger on the same words, and a chief that had two would answer for
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    Core,
    CoreHtml,
    Tdd,
    TddHtml,
}

impl Variant {
    /// Every variant, in the order the page offers them.
    pub const ALL: [Variant; 4] = [
        Variant::Core,
        Variant::CoreHtml,
        Variant::Tdd,
        Variant::TddHtml,
    ];

    /// The variant a project plans with unless it names another.
    pub const DEFAULT: Variant = Variant::Core;

    /// The name the ledger, the page and the live tests say it by (`core-html`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Variant::Core => "core",
            Variant::CoreHtml => "core-html",
            Variant::Tdd => "tdd",
            Variant::TddHtml => "tdd-html",
        }
    }

    /// The variant a name says; none for any other word.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|variant| variant.as_str() == name)
    }

    /// The name the variant's skill carries in the frontmatter of its
    /// `SKILL.md` (`specmint-core-html`), and its folder is named by.
    pub const fn skill_name(self) -> &'static str {
        match self {
            Variant::Core => "specmint-core",
            Variant::CoreHtml => "specmint-core-html",
            Variant::Tdd => "specmint-tdd",
            Variant::TddHtml => "specmint-tdd-html",
        }
    }
}

/// What a project says of specs on its own, each part left unsaid where the
/// human named nothing: whether they are on (none follows the app's default),
/// the folder (none is [`DEFAULT_FOLDER`]) and the variant (none is
/// [`Variant::DEFAULT`]). The folder is as the ledger keeps it: relative to the
/// project, in slashes, with no `..`. Said by a project's view only when it
/// says anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecsView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// What a chief's launch is given where specs are on: where the specs are
/// kept and the one Specmint variant it plans with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecsSetting {
    pub folder: String,
    pub variant: Variant,
}

impl SpecsView {
    /// The view, or none where it says nothing: a project's view holds specs
    /// only when the human said something of them.
    pub fn said(self) -> Option<Self> {
        (self != Self::default()).then_some(self)
    }

    /// The setting a chief's launch is given, where `own` is what the project
    /// says and `app_default` is whether the human turned specs on for every
    /// project: none where they are off. A project's own word wins either way;
    /// what it leaves unsaid is the default's (the app default for whether, the
    /// documented folder and variant for the rest).
    pub(crate) fn resolve(own: Option<&SpecsView>, app_default: bool) -> Option<SpecsSetting> {
        let on = own.and_then(|own| own.on).unwrap_or(app_default);
        on.then(|| SpecsSetting {
            folder: own
                .and_then(|own| own.folder.clone())
                .unwrap_or_else(|| DEFAULT_FOLDER.to_owned()),
            variant: own
                .and_then(|own| own.variant.as_deref())
                .and_then(Variant::from_name)
                .unwrap_or(Variant::DEFAULT),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variant_says_its_name_and_its_skill_and_a_name_says_one_variant() {
        let named: Vec<_> = Variant::ALL
            .into_iter()
            .map(|variant| (variant.as_str(), variant.skill_name()))
            .collect();
        assert_eq!(
            named,
            [
                ("core", "specmint-core"),
                ("core-html", "specmint-core-html"),
                ("tdd", "specmint-tdd"),
                ("tdd-html", "specmint-tdd-html"),
            ]
        );
        for variant in Variant::ALL {
            assert_eq!(Variant::from_name(variant.as_str()), Some(variant));
        }
        for word in ["", "Core", "core ", "specmint-core", "html", "tdd_html"] {
            assert_eq!(Variant::from_name(word), None, "{word:?}");
        }
        assert_eq!(Variant::DEFAULT, Variant::Core);
    }

    #[test]
    fn a_view_says_only_what_the_project_named() {
        let written = |view: &SpecsView| serde_json::to_string(view).unwrap();
        assert_eq!(written(&SpecsView::default()), "{}");
        let named = SpecsView {
            on: Some(false),
            folder: Some("docs/specs".to_owned()),
            variant: Some("tdd".to_owned()),
        };
        assert_eq!(
            written(&named),
            r#"{"on":false,"folder":"docs/specs","variant":"tdd"}"#
        );
    }

    fn own(on: Option<bool>) -> SpecsView {
        SpecsView {
            on,
            folder: Some("plans".to_owned()),
            variant: Some("tdd-html".to_owned()),
        }
    }

    #[test]
    fn the_project_s_word_on_whether_wins_and_none_follows_the_app_default() {
        for (own_word, app_default, on) in [
            (None, false, false),
            (None, true, true),
            (Some(true), false, true),
            (Some(true), true, true),
            (Some(false), false, false),
            (Some(false), true, false),
        ] {
            let resolved = SpecsView::resolve(Some(&own(own_word)), app_default);
            assert_eq!(resolved.is_some(), on, "{own_word:?} over {app_default}");
        }
    }

    #[test]
    fn what_the_project_leaves_unsaid_is_the_documented_default() {
        assert_eq!(SpecsView::resolve(None, false), None);
        assert_eq!(
            SpecsView::resolve(None, true),
            Some(SpecsSetting {
                folder: ".specs".to_owned(),
                variant: Variant::Core,
            })
        );
        assert_eq!(
            SpecsView::resolve(Some(&SpecsView::default()), true),
            SpecsView::resolve(None, true)
        );
        assert_eq!(
            SpecsView::resolve(Some(&own(Some(true))), false),
            Some(SpecsSetting {
                folder: "plans".to_owned(),
                variant: Variant::TddHtml,
            })
        );
    }
}
