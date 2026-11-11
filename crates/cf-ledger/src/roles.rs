//! The staff's roles, one row a role: the id the ledger stores, the name the texts and the
//! refusals say, the switch of `cf task add` that asks for it, whether its work is given by
//! tier, whether only an image agent holds it, and whether its work changes the project's
//! files. A row's place in the table is the role's place in every list of roles: the
//! refusals, the staff page and its pickers. Every reader of a role asks this table, and
//! no other place compares a role's id with a word of its own.
//!
//! What it does not hold: the SQLite CHECKs of `participant.role` and `task.pool`
//! (`migrations/0012.sql`), which only a migration writes; each role's text
//! (`skill/core/staff.md`) and the chief's lines about it (`skill/core/chief.md`); and the
//! prose that names every role (`cf task add`'s usage, the staff page's help).

/// One role of the staff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Role {
    /// The role as the ledger stores it: in `participant.role` and `participant.roles`,
    /// and as `task.pool`, the role a task is for.
    pub id: &'static str,
    /// The role as the texts and the refusals say it: `worker`, `image designer`.
    pub name: &'static str,
    /// The article of the name said alone: `an image designer`. A role given by tier is
    /// said after its tier (`a standard worker`), whose article is the tier's.
    pub article: &'static str,
    /// The switch of `cf task add` that asks for it, `--review`; its words without the
    /// dashes are the field of `POST /api/tasks` that does. None for the role a task is
    /// for when it asks for none: the worker.
    pub switch: Option<&'static str>,
    /// Whether its tasks are given by tier, and its members are told theirs. A role with
    /// none (the image designer) takes a task with no tier, and every member of it may.
    pub tiered: bool,
    /// Whether only an image agent holds it: an image agent (a Codex agent that draws)
    /// holds this role and nothing else, and no other agent holds it.
    pub image_agent: bool,
    /// Whether its work writes into the project's files: the worker's work and the image
    /// designer's images do; advice, a review and a test change none of them (a test's
    /// own script aside, where its task asks for one).
    pub changes_files: bool,
}

impl Role {
    /// The field of `POST /api/tasks` that asks for it: its switch without the dashes.
    pub fn field(&self) -> Option<&'static str> {
        self.switch.map(|switch| switch.trim_start_matches('-'))
    }

    /// The name said alone, with its article: `an image designer`.
    pub fn a_name(&self) -> String {
        format!("{} {}", self.article, self.name)
    }

    /// The name as a label begins it: `Image designer`.
    pub fn label(&self) -> String {
        let mut letters = self.name.chars();
        letters.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(letters).collect()
        })
    }
}

/// The worker: the work itself, given by tier; a task that asks for no role is its.
pub const WORKER: Role = Role {
    id: "worker",
    name: "worker",
    article: "a",
    switch: None,
    tiered: true,
    image_agent: false,
    changes_files: true,
};

/// The frontend designer: how the product looks, feels and moves, designed and built, with
/// no tier.
pub const FRONTEND: Role = Role {
    id: "frontend",
    name: "frontend designer",
    article: "a",
    switch: Some("--frontend"),
    tiered: false,
    image_agent: false,
    changes_files: true,
};

/// The architect: the software's design written down as files (documents, contracts and the
/// code's frame) before it is built, with no tier.
pub const ARCHITECT: Role = Role {
    id: "architect",
    name: "architect",
    article: "an",
    switch: Some("--architect"),
    tiered: false,
    image_agent: false,
    changes_files: true,
};

/// The researcher: what the code, the project's documents and the web say, looked up and
/// reported, with no tier.
pub const RESEARCHER: Role = Role {
    id: "researcher",
    name: "researcher",
    article: "a",
    switch: Some("--research"),
    tiered: false,
    image_agent: false,
    changes_files: false,
};

/// The advisor: advice for the chief, which only the chief asks for.
pub const ADVISOR: Role = Role {
    id: "advisor",
    name: "advisor",
    article: "an",
    switch: Some("--advice"),
    tiered: true,
    image_agent: false,
    changes_files: false,
};

/// The reviewer: a review of finished work.
pub const REVIEWER: Role = Role {
    id: "reviewer",
    name: "reviewer",
    article: "a",
    switch: Some("--review"),
    tiered: true,
    image_agent: false,
    changes_files: false,
};

/// The tester: finished work checked by running it.
pub const TESTER: Role = Role {
    id: "tester",
    name: "tester",
    article: "a",
    switch: Some("--test"),
    tiered: true,
    image_agent: false,
    changes_files: false,
};

/// The integrator: accepted work merged, checked and landed, with no tier.
pub const INTEGRATOR: Role = Role {
    id: "integrator",
    name: "integrator",
    article: "an",
    switch: Some("--integrate"),
    tiered: false,
    image_agent: false,
    changes_files: true,
};

/// The image designer: an image drawn on Codex's image tool, with no tier.
pub const IMAGE_DESIGNER: Role = Role {
    id: "designer",
    name: "image designer",
    article: "an",
    switch: Some("--design"),
    tiered: false,
    image_agent: true,
    changes_files: true,
};

/// The roles in their order: the ones that make the work (the worker, the frontend designer,
/// the architect) and the researcher, then the ones that weigh and check it (the advisor, the
/// reviewer, the tester), the integrator who lands it, and the image designer last (the staff
/// page's order, 2026-10-24; the four of spec new-roles, 2026-11-11).
const ROLES: [Role; 9] = [
    WORKER,
    FRONTEND,
    ARCHITECT,
    RESEARCHER,
    ADVISOR,
    REVIEWER,
    TESTER,
    INTEGRATOR,
    IMAGE_DESIGNER,
];

/// Every role, in its order.
pub fn all() -> &'static [Role] {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(table) = seam::table() {
        return table;
    }
    &ROLES
}

/// The role whose id is `id`; none for a word that is no role (the chief, the human).
pub fn find(id: &str) -> Option<&'static Role> {
    all().iter().find(|role| role.id == id)
}

/// Whether `id` is a role of the staff: a member's, or a pool a task is for.
pub fn is_role(id: &str) -> bool {
    find(id).is_some()
}

/// The roles' ids, in their order.
pub fn ids() -> Vec<&'static str> {
    all().iter().map(|role| role.id).collect()
}

/// Whether `pool` is a role whose tasks take no tier.
pub fn untiered(pool: &str) -> bool {
    find(pool).is_some_and(|role| !role.tiered)
}

/// Whether a member that holds `roles` takes no tier: it holds a role, and none of its roles
/// has tiers. One that holds a role with tiers beside them keeps its tier, for that role's work.
pub fn takes_no_tier<S: AsRef<str>>(roles: &[S]) -> bool {
    !roles.is_empty() && roles.iter().all(|role| untiered(role.as_ref()))
}

/// The tier the ledger keeps a member in that takes none: the lowest, which decides nothing,
/// since none of its tasks has a tier (the image designer's since designer P2).
pub const NO_TIER: &str = "light";

/// Whether one of `roles` is a role only an image agent holds.
pub fn needs_an_image_agent<S: AsRef<str>>(roles: &[S]) -> bool {
    roles
        .iter()
        .any(|role| find(role.as_ref()).is_some_and(|role| role.image_agent))
}

/// The role a task asks for by the fields that are `on`: a role with no tier before one
/// with tiers (`--design` stands alone, whatever else is asked), then the table's order;
/// the role with no switch, the worker, where none is on.
pub fn asked(on: impl Fn(&str) -> bool) -> &'static Role {
    let table = all();
    let flagged = |tiered: bool| {
        table
            .iter()
            .filter(move |role| role.tiered == tiered)
            .find(|role| role.field().is_some_and(&on))
    };
    flagged(false)
        .or_else(|| flagged(true))
        .or_else(|| table.iter().find(|role| role.switch.is_none()))
        .unwrap_or(&WORKER)
}

/// How a refusal names `pool`: its role's name, or the word itself when it is no role.
pub fn name_of(pool: &str) -> &str {
    find(pool).map_or(pool, |role| role.name)
}

/// "standard worker", "image designer": who an open task waits for, by its pool and its
/// tier as the ledger holds them. A role with no tier is its name alone; any other pool is
/// said after its tier, and a pool or a tier that is not there as `null`, as JavaScript
/// printed it.
pub fn pool_name(pool: Option<&str>, tier: Option<&str>) -> String {
    match pool.and_then(find) {
        Some(role) if !role.tiered => role.name.to_owned(),
        _ => format!(
            "{} {}",
            tier.unwrap_or("null"),
            pool.map_or("null", name_of)
        ),
    }
}

/// The same with its article: `a standard worker`, `an image designer`.
pub fn a_pool(pool: Option<&str>, tier: Option<&str>) -> String {
    match pool.and_then(find) {
        Some(role) if !role.tiered => role.a_name(),
        _ => format!("a {}", pool_name(pool, tier)),
    }
}

/// What the page reads of the roles, in their order: each one's id, its name and the label
/// the page begins it with, its article, whether it has tiers and whether only an image
/// agent holds it.
pub fn page_view() -> serde_json::Value {
    all()
        .iter()
        .map(|role| {
            serde_json::json!({
                "id": role.id,
                "name": role.name,
                "label": role.label(),
                "article": role.article,
                "tiered": role.tiered,
                "imageAgent": role.image_agent,
            })
        })
        .collect()
}

/// The tests' way to put another table in this one's place: on this thread, while `run`
/// runs. A table that adds a role shows every reader takes it from here; the SQLite CHECKs
/// of `participant.role` and `task.pool` hold the table's roles alone, so a test that writes
/// a row of a role they do not name turns them off on its own connection.
#[cfg(any(test, feature = "test-support"))]
pub mod seam {
    use std::cell::Cell;

    use super::Role;

    thread_local! {
        static TABLE: Cell<Option<&'static [Role]>> = const { Cell::new(None) };
    }

    pub(super) fn table() -> Option<&'static [Role]> {
        TABLE.with(Cell::get)
    }

    /// A sixth role, which only the tests hold: a field scout, given no tier, held by any
    /// agent but an image agent, named apart from its id, between the reviewer and the
    /// tester.
    pub const SCOUT: Role = Role {
        id: "scout",
        name: "field scout",
        article: "a",
        switch: Some("--scout"),
        tiered: false,
        image_agent: false,
        changes_files: false,
    };

    /// The five roles the table began with (the registry's, FD-03), with the scout in its
    /// place: what the registry's own tests of a role in the table alone were written on.
    pub static WITH_A_SCOUT: [Role; 6] = [
        super::WORKER,
        super::ADVISOR,
        super::REVIEWER,
        SCOUT,
        super::TESTER,
        super::IMAGE_DESIGNER,
    ];

    /// `run`, with the scout among the roles on this thread.
    pub fn with_a_scout<T>(run: impl FnOnce() -> T) -> T {
        with_table(&WITH_A_SCOUT, run)
    }

    /// `run`, with `table` as the roles on this thread; the table before it again after,
    /// even if `run` panics.
    pub fn with_table<T>(table: &'static [Role], run: impl FnOnce() -> T) -> T {
        let _put = put(table);
        run()
    }

    /// `table` as the roles on this thread until what this returns is dropped: for a test
    /// whose work goes on across `.await`s on a runtime of this one thread.
    #[must_use = "the table goes back as soon as this is dropped"]
    pub fn put(table: &'static [Role]) -> Put {
        Put(TABLE.with(|cell| cell.replace(Some(table))))
    }

    /// The table [`put`] replaced, put back on drop.
    pub struct Put(Option<&'static [Role]>);

    impl Drop for Put {
        fn drop(&mut self) {
            TABLE.with(|cell| cell.set(self.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roles_are_the_nine_in_the_staff_pages_order() {
        assert_eq!(
            ids(),
            [
                "worker",
                "frontend",
                "architect",
                "researcher",
                "advisor",
                "reviewer",
                "tester",
                "integrator",
                "designer"
            ]
        );
        let names: Vec<String> = all().iter().map(Role::a_name).collect();
        assert_eq!(
            names,
            [
                "a worker",
                "a frontend designer",
                "an architect",
                "a researcher",
                "an advisor",
                "a reviewer",
                "a tester",
                "an integrator",
                "an image designer"
            ]
        );
        let labels: Vec<String> = all().iter().map(Role::label).collect();
        assert_eq!(
            labels,
            [
                "Worker",
                "Frontend designer",
                "Architect",
                "Researcher",
                "Advisor",
                "Reviewer",
                "Tester",
                "Integrator",
                "Image designer"
            ]
        );
        let switches: Vec<Option<&str>> = all().iter().map(|role| role.switch).collect();
        assert_eq!(
            switches,
            [
                None,
                Some("--frontend"),
                Some("--architect"),
                Some("--research"),
                Some("--advice"),
                Some("--review"),
                Some("--test"),
                Some("--integrate"),
                Some("--design")
            ]
        );
        let untiered: Vec<&str> = all()
            .iter()
            .filter(|role| !role.tiered)
            .map(|role| role.id)
            .collect();
        assert_eq!(
            untiered,
            ["frontend", "architect", "researcher", "integrator", "designer"]
        );
        assert_eq!(
            all()
                .iter()
                .filter(|role| role.image_agent)
                .collect::<Vec<_>>(),
            [&IMAGE_DESIGNER]
        );
        let writers: Vec<&str> = all()
            .iter()
            .filter(|role| role.changes_files)
            .map(|role| role.id)
            .collect();
        assert_eq!(
            writers,
            ["worker", "frontend", "architect", "integrator", "designer"]
        );
        // The two designers are told apart by their names wherever they are said.
        assert_eq!(name_of("designer"), "image designer");
        assert_eq!(name_of("frontend"), "frontend designer");
    }

    #[test]
    fn a_task_asks_for_the_image_designer_before_anything_else_then_advice_review_and_test() {
        let asked_by = |fields: &[&str]| asked(|field| fields.contains(&field)).id;
        assert_eq!(asked_by(&[]), "worker");
        assert_eq!(asked_by(&["tier", "purpose"]), "worker");
        assert_eq!(
            asked_by(&["test", "review", "advice", "design"]),
            "designer"
        );
        assert_eq!(asked_by(&["test", "review", "advice"]), "advisor");
        assert_eq!(asked_by(&["test", "review"]), "reviewer");
        assert_eq!(asked_by(&["test"]), "tester");
        // Each of the four untiered roles is asked for by its own field, before any with tiers.
        assert_eq!(asked_by(&["frontend"]), "frontend");
        assert_eq!(asked_by(&["architect", "advice"]), "architect");
        assert_eq!(asked_by(&["research", "review"]), "researcher");
        assert_eq!(asked_by(&["integrate", "test"]), "integrator");
        assert_eq!(asked_by(&["integrate"]), "integrator");
    }

    #[test]
    fn an_open_task_is_said_by_its_tier_and_role_and_an_untiered_one_by_its_name() {
        assert_eq!(
            pool_name(Some("worker"), Some("standard")),
            "standard worker"
        );
        assert_eq!(
            a_pool(Some("advisor"), Some("complex")),
            "a complex advisor"
        );
        assert_eq!(a_pool(Some("designer"), None), "an image designer");
        assert_eq!(a_pool(Some("designer"), Some("light")), "an image designer");
        assert_eq!(pool_name(None, None), "null null");
        assert_eq!(a_pool(Some("chief"), Some("light")), "a light chief");
        assert_eq!(name_of("designer"), "image designer");
        assert_eq!(name_of("reviewer"), "reviewer");
        assert_eq!(name_of("member"), "member");
    }

    #[test]
    fn a_table_put_in_place_for_a_test_holds_on_its_thread_and_goes_after() {
        static SIX: [Role; 2] = [
            WORKER,
            Role {
                id: "sixth",
                name: "sixth hand",
                article: "a",
                switch: Some("--sixth"),
                tiered: false,
                image_agent: false,
                changes_files: false,
            },
        ];
        seam::with_table(&SIX, || {
            assert_eq!(ids(), ["worker", "sixth"]);
            assert_eq!(asked(|field| field == "sixth").id, "sixth");
            assert!(!is_role("designer"));
            assert!(std::thread::spawn(|| is_role("designer")).join().unwrap());
        });
        assert!(is_role("designer"));
        assert!(!is_role("sixth"));
    }
}
