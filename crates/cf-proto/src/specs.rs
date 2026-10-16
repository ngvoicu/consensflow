//! Specs, as a project and the app say them. A project's chief may plan big
//! work as specs (Specmint's: a folder of phased plans) and carry each spec's
//! phases onto the board as tasks. Whether it does is the human's choice, off
//! unless they turn it on: for every project at once, with the folder and the
//! variant every project uses unless it names its own (what the agents file
//! keeps, [`AppSpecs`]), and for one project on its own, which says its own
//! word on whether, the folder it keeps its specs in and the one Specmint
//! variant it plans with ([`SpecsView`]). [`ProjectView::specs_setting`] reads
//! the two together: a project's word first, then the app's, then the
//! documented default.
//!
//! [`ProjectView::specs_setting`]: crate::ledger::ProjectView::specs_setting

pub mod check;

use serde::{Serialize, Serializer};

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

    /// The variant a project plans with unless it, or the app, names another.
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

    /// A few words on what the variant is, as Settings says them beside its name.
    pub const fn about(self) -> &'static str {
        match self {
            Variant::Core => "Markdown specs",
            Variant::CoreHtml => "HTML spec pages",
            Variant::Tdd => "Test-first Markdown specs",
            Variant::TddHtml => "Test-first specs as HTML pages",
        }
    }
}

/// A variant is said by its name (`core-html`), as the ledger and the page say it.
impl Serialize for Variant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// What a project says of specs on its own, each part left unsaid where the
/// human named nothing, which then follows what the app says ([`AppSpecs`]):
/// whether they are on, the folder and the variant. The folder is as the ledger
/// keeps it: relative to the project, in slashes, with no `..`. Said by a
/// project's view only when it says anything.
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

/// What the human says of specs for every project that does not say otherwise:
/// whether they are on, and the folder and the variant a project uses when it
/// names none (none is [`DEFAULT_FOLDER`] and [`Variant::DEFAULT`]). The agents
/// file keeps the folder and the variant only where they are set, so a file
/// that never knew of them reads as it always did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AppSpecs {
    pub on: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<Variant>,
}

impl AppSpecs {
    /// The folder a project that names none keeps its specs in: the one the app
    /// names, else [`DEFAULT_FOLDER`].
    pub fn folder_or_default(&self) -> &str {
        self.folder.as_deref().unwrap_or(DEFAULT_FOLDER)
    }

    /// The variant a project that names none plans with: the one the app names,
    /// else [`Variant::DEFAULT`].
    pub fn variant_or_default(&self) -> Variant {
        self.variant.unwrap_or(Variant::DEFAULT)
    }
}

/// One variant of the Specmint this build bundles, as Settings shows it: read
/// from the bundled files, never typed. What the files do not say is none
/// (`null`), and the page says it is unknown: it is no reason to keep the human's
/// settings from them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BundledVariant {
    /// The variant's name (`core-html`).
    pub name: String,
    /// The `version` its `SKILL.md` says, where it says one.
    pub version: Option<String>,
    /// The upstream commit it is vendored at, short, where the manifest pins one.
    pub commit: Option<String>,
    /// A few words on what it is ([`Variant::about`]).
    pub about: String,
}

/// The Specmint this build bundles: its version, and each variant in the order
/// the page offers them. The version is the one every variant says, or the
/// versions they say, in that order, where they do not agree; none where no
/// variant says one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bundled {
    pub version: Option<String>,
    pub variants: Vec<BundledVariant>,
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
    /// says and `app` is what the human says for every project: none where
    /// specs are off. A project's own word wins either way; what it leaves
    /// unsaid is the app's, and what the app leaves unsaid is the documented
    /// folder and variant.
    pub(crate) fn resolve(own: Option<&SpecsView>, app: &AppSpecs) -> Option<SpecsSetting> {
        let on = own.and_then(|own| own.on).unwrap_or(app.on);
        on.then(|| SpecsSetting {
            folder: own
                .and_then(|own| own.folder.clone())
                .unwrap_or_else(|| app.folder_or_default().to_owned()),
            variant: own
                .and_then(|own| own.variant.as_deref())
                .and_then(Variant::from_name)
                .unwrap_or_else(|| app.variant_or_default()),
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

    /// What the human says for every project: on or off, and no folder or variant.
    fn app(on: bool) -> AppSpecs {
        AppSpecs {
            on,
            ..AppSpecs::default()
        }
    }

    /// What the human says for every project, all three parts.
    fn app_with(folder: &str, variant: Variant) -> AppSpecs {
        AppSpecs {
            on: true,
            folder: Some(folder.to_owned()),
            variant: Some(variant),
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
            let resolved = SpecsView::resolve(Some(&own(own_word)), &app(app_default));
            assert_eq!(resolved.is_some(), on, "{own_word:?} over {app_default}");
        }
    }

    #[test]
    fn what_neither_the_project_nor_the_app_says_is_the_documented_default() {
        assert_eq!(SpecsView::resolve(None, &app(false)), None);
        assert_eq!(
            SpecsView::resolve(None, &app(true)),
            Some(SpecsSetting {
                folder: ".specs".to_owned(),
                variant: Variant::Core,
            })
        );
        assert_eq!(
            SpecsView::resolve(Some(&SpecsView::default()), &app(true)),
            SpecsView::resolve(None, &app(true))
        );
    }

    #[test]
    fn the_project_s_word_wins_over_the_app_s_for_the_folder_and_for_the_variant() {
        assert_eq!(
            SpecsView::resolve(Some(&own(Some(true))), &app_with("docs", Variant::Tdd)),
            Some(SpecsSetting {
                folder: "plans".to_owned(),
                variant: Variant::TddHtml,
            })
        );
    }

    #[test]
    fn what_the_project_leaves_unsaid_is_the_app_s_part_by_part_and_never_the_default_over_it() {
        let app = app_with("docs/specs", Variant::CoreHtml);
        let says = |folder: Option<&str>, variant: Option<&str>| SpecsView {
            on: Some(true),
            folder: folder.map(str::to_owned),
            variant: variant.map(str::to_owned),
        };
        let resolved = |own: &SpecsView| {
            let setting = SpecsView::resolve(Some(own), &app).unwrap();
            (setting.folder, setting.variant)
        };
        assert_eq!(
            resolved(&says(None, None)),
            ("docs/specs".to_owned(), Variant::CoreHtml)
        );
        assert_eq!(
            resolved(&says(Some("plans"), None)),
            ("plans".to_owned(), Variant::CoreHtml)
        );
        assert_eq!(
            resolved(&says(None, Some("tdd"))),
            ("docs/specs".to_owned(), Variant::Tdd)
        );
        // A project with no word of its own follows the app all the way.
        assert_eq!(
            SpecsView::resolve(None, &app),
            SpecsView::resolve(Some(&SpecsView::default()), &app)
        );
        // An app that names only one part leaves the other to the default.
        let only_folder = AppSpecs {
            on: true,
            folder: Some("docs".to_owned()),
            variant: None,
        };
        assert_eq!(
            SpecsView::resolve(None, &only_folder),
            Some(SpecsSetting {
                folder: "docs".to_owned(),
                variant: Variant::Core,
            })
        );
    }

    #[test]
    fn a_variant_says_what_it_is_in_a_few_words_and_the_app_says_only_what_was_set() {
        let about: Vec<_> = Variant::ALL.map(Variant::about).to_vec();
        assert_eq!(
            about,
            [
                "Markdown specs",
                "HTML spec pages",
                "Test-first Markdown specs",
                "Test-first specs as HTML pages",
            ]
        );
        let written = |app: &AppSpecs| serde_json::to_string(app).unwrap();
        assert_eq!(written(&AppSpecs::default()), r#"{"on":false}"#);
        assert_eq!(
            written(&app_with("docs/specs", Variant::TddHtml)),
            r#"{"on":true,"folder":"docs/specs","variant":"tdd-html"}"#
        );
    }
}
