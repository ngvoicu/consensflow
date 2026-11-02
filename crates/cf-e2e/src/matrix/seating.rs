//! Who sits where: the five positions of a board, the harness in each, and how they are put
//! there. A board is one row of the matrix's rotation (LMX-03's design, section 3): the chief
//! and one member in each of the worker's, the reviewer's, the advisor's and the tester's
//! roles, each member in that one role alone and every one of them at `standard`, so that each
//! pool has one member and a task of the pool can land on no other. The members are added as
//! the human adds staff ([`Members::add`]), and the tier each is given is checked.
//!
//! The stand-in seating ([`Seating::stand_ins`]) is the same five positions played by the
//! rig's stand-in agent: the control the runner's driver and oracle are proved on offline. It
//! is no coverage of any harness.

use serde_json::{json, Value};

use crate::live::agents::{self, STAND_IN_CHIEF_MODEL, STAND_IN_WORKER_MODEL};
use crate::live::members::{Members, Plan, BRIEF};
use crate::live::report::Check;
use crate::live::Harness;
use crate::{Error, Result};

/// The tier every member of a board is at: one member to a pool, and none to fall back to.
pub const TIER: &str = "standard";

/// A position of a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Chief,
    Worker,
    Reviewer,
    Advisor,
    Tester,
}

impl Role {
    /// Every position, in the order the design's table has them.
    pub const ALL: [Self; 5] = [
        Self::Chief,
        Self::Worker,
        Self::Reviewer,
        Self::Advisor,
        Self::Tester,
    ];

    /// Its name, as the evidence says it (and as the ledger names a member's role).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Chief => "chief",
            Self::Worker => "worker",
            Self::Reviewer => "reviewer",
            Self::Advisor => "advisor",
            Self::Tester => "tester",
        }
    }

    /// The pool a task for this position is put in (`cf task add`, `--review`, `--advice`,
    /// `--test`): none for the chief, which is given no task by a pool.
    pub const fn pool(self) -> Option<&'static str> {
        match self {
            Self::Chief => None,
            Self::Worker => Some("worker"),
            Self::Reviewer => Some("reviewer"),
            Self::Advisor => Some("advisor"),
            Self::Tester => Some("tester"),
        }
    }

    /// The id of the agent that sits here, in the roster of the run's home.
    pub fn agent(self) -> String {
        format!("matrix-{}", self.name())
    }
}

/// A position and who sits in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub role: Role,
    /// The real harness, or none where the rig's stand-in plays it.
    pub harness: Option<Harness>,
}

impl Seat {
    /// The id of its agent.
    pub fn agent(&self) -> String {
        self.role.agent()
    }

    /// The kind of record its windows keep, as `harness-ask records` reads it: the stand-in
    /// keeps Claude Code's.
    pub fn kind(&self) -> &'static str {
        self.harness.map_or("claude-code", Harness::kind)
    }

    /// Who it is, as the evidence says it: the harness's name, or `stand-in`.
    pub fn who(&self) -> &'static str {
        self.harness.map_or("stand-in", Harness::name)
    }

    /// Its roster row: the harness on the kit's model for it ([`agents::model`]: the cheap
    /// one, or what `--models` names), or the stand-in's, at [`TIER`] and under the
    /// position's own id.
    pub fn row(&self) -> Value {
        let mut row = match self.harness {
            Some(harness) => agents::row_on(&agents::model(harness), TIER),
            None => {
                let model = if self.role == Role::Chief {
                    STAND_IN_CHIEF_MODEL
                } else {
                    STAND_IN_WORKER_MODEL
                };
                json!({ "kind": "claude-code", "model": model, "workTier": TIER })
            }
        };
        if let Some(fields) = row.as_object_mut() {
            fields.insert("id".to_owned(), json!(self.agent()));
        }
        row
    }
}

/// The harness of each position of board 0 (LMX-03's table, its first row).
pub const BOARD_0: [(Role, Harness); 5] = [
    (Role::Chief, Harness::Claude),
    (Role::Worker, Harness::Codex),
    (Role::Reviewer, Harness::Pi),
    (Role::Advisor, Harness::Devin),
    (Role::Tester, Harness::Muse),
];

/// The five positions of a board and who sits in each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seating {
    /// The board's number; none for the stand-ins' control.
    pub board: Option<u8>,
    pub seats: Vec<Seat>,
}

impl Seating {
    /// Board `number` of the rotation. Only board 0 is built in this slice (LMX-04's row 1);
    /// the others are LMX-05's, and are refused rather than guessed.
    pub fn board(number: u8) -> Result<Self> {
        if number != 0 {
            return Err(Error::Daemon(format!(
                "the matrix has board 0 alone so far (chief Claude Code, worker Codex, reviewer Pi, advisor Devin, tester Muse Code): no board {number}"
            )));
        }
        Ok(Self {
            board: Some(0),
            seats: BOARD_0
                .iter()
                .map(|(role, harness)| Seat {
                    role: *role,
                    harness: Some(*harness),
                })
                .collect(),
        })
    }

    /// The five positions, each played by the rig's stand-in.
    pub fn stand_ins() -> Self {
        Self {
            board: None,
            seats: Role::ALL
                .iter()
                .map(|role| Seat {
                    role: *role,
                    harness: None,
                })
                .collect(),
        }
    }

    /// The seat of `role`.
    pub fn seat(&self, role: Role) -> &Seat {
        self.seats
            .iter()
            .find(|seat| seat.role == role)
            .unwrap_or_else(|| panic!("a seating has every position, and {} is none", role.name()))
    }

    /// The seat whose agent is `agent`, where one is.
    pub fn of_agent(&self, agent: &str) -> Option<&Seat> {
        self.seats.iter().find(|seat| seat.agent() == agent)
    }

    /// Whether the chief is the stand-in, which is told what to do in its own words.
    pub fn chief_is_stand_in(&self) -> bool {
        self.seat(Role::Chief).harness.is_none()
    }

    /// The project the board opens: the folder `folder`, told the kit's brief (do what each
    /// message asks; a message that names a tool or a command is asking for it), the files
    /// `files`, and the roster of the five seats, the chief first.
    pub fn plan(&self, folder: &str, files: &[(&str, &str)]) -> Plan {
        let mut plan = Plan::new(folder, self.seat(Role::Chief).row()).told(BRIEF);
        for seat in self.seats.iter().filter(|seat| seat.role != Role::Chief) {
            plan = plan.with(seat.row());
        }
        for (path, text) in files {
            plan = plan.with_file(path, *text);
        }
        plan
    }

    /// The human's staff setup: each member added with its one role, as the page adds it,
    /// and a check of each, that it was added and at [`TIER`].
    pub fn staff(&self, members: &mut Members) -> Vec<Check> {
        let mut checks = Vec::new();
        for seat in self.seats.iter().filter(|seat| seat.role != Role::Chief) {
            let what = format!("{} {} is staff at {TIER}", seat.role.name(), seat.who());
            let role = seat.role.pool().unwrap_or_default();
            checks.push(match members.add(&seat.agent(), &[role]) {
                Ok(tier) => Check::new(
                    "staff",
                    &what,
                    tier == TIER,
                    format!("member.add gave {} the tier {tier:?}", seat.agent()),
                ),
                Err(failed) => Check::failed("staff", &what, failed.to_string()),
            });
        }
        checks
    }

    /// The seats, as the evidence's manifest has them.
    pub fn manifest(&self) -> Value {
        Value::Array(
            self.seats
                .iter()
                .map(|seat| {
                    json!({
                        "role": seat.role.name(),
                        "harness": seat.who(),
                        "agent": seat.agent(),
                        "row": seat.row(),
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_0_is_the_designs_first_row_and_every_other_board_is_refused() {
        let board = Seating::board(0).unwrap();
        let seated: Vec<(&str, &str)> = board
            .seats
            .iter()
            .map(|seat| (seat.role.name(), seat.who()))
            .collect();
        assert_eq!(
            seated,
            [
                ("chief", "claude"),
                ("worker", "codex"),
                ("reviewer", "pi"),
                ("advisor", "devin"),
                ("tester", "muse")
            ]
        );
        let refused = Seating::board(1).unwrap_err().to_string();
        assert!(refused.contains("board 0 alone so far"), "{refused}");
    }

    #[test]
    fn each_harness_sits_in_one_position_alone_on_an_agent_of_that_position() {
        let board = Seating::board(0).unwrap();
        let mut harnesses: Vec<&str> = board.seats.iter().map(Seat::who).collect();
        harnesses.sort_unstable();
        harnesses.dedup();
        assert_eq!(harnesses.len(), 5);
        let ids: Vec<String> = board.seats.iter().map(Seat::agent).collect();
        assert_eq!(
            ids,
            [
                "matrix-chief",
                "matrix-worker",
                "matrix-reviewer",
                "matrix-advisor",
                "matrix-tester"
            ]
        );
        assert_eq!(board.of_agent("matrix-tester").unwrap().who(), "muse");
        assert_eq!(board.of_agent("live-muse"), None);
    }

    #[test]
    fn every_member_is_at_standard_on_the_kits_model_for_its_harness() {
        let board = Seating::board(0).unwrap();
        for seat in &board.seats {
            let row = seat.row();
            let model = agents::model(seat.harness.unwrap());
            assert_eq!(row["id"], seat.agent());
            assert_eq!(row["workTier"], TIER);
            assert_eq!(row["kind"], seat.kind());
            assert_eq!(row["model"], model.name.as_str(), "{row}");
        }
        // The kit's own model for Devin carries its effort, and Pi's none.
        assert_eq!(board.seat(Role::Advisor).row()["effort"], "medium");
    }

    #[test]
    fn a_position_has_its_pool_and_the_chief_none() {
        let pools: Vec<Option<&str>> = Role::ALL.iter().map(|role| role.pool()).collect();
        assert_eq!(
            pools,
            [
                None,
                Some("worker"),
                Some("reviewer"),
                Some("advisor"),
                Some("tester")
            ]
        );
    }

    #[test]
    fn the_stand_ins_play_every_position_as_the_rigs_claude_and_the_chief_as_its_chief() {
        let control = Seating::stand_ins();
        assert!(control.chief_is_stand_in());
        assert_eq!(control.board, None);
        assert_eq!(
            control.seat(Role::Chief).row()["model"],
            STAND_IN_CHIEF_MODEL
        );
        assert_eq!(
            control.seat(Role::Tester).row()["model"],
            STAND_IN_WORKER_MODEL
        );
        assert!(control
            .seats
            .iter()
            .all(|seat| seat.kind() == "claude-code" && seat.who() == "stand-in"));
        assert!(!Seating::board(0).unwrap().chief_is_stand_in());
    }

    #[test]
    fn the_plan_has_the_chief_first_then_each_member_and_the_files() {
        let plan = Seating::stand_ins().plan("matrix-x", &[("greet.cjs", "a")]);
        assert_eq!(plan.chief, "matrix-chief");
        let ids: Vec<&str> = plan
            .roster
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            [
                "matrix-chief",
                "matrix-worker",
                "matrix-reviewer",
                "matrix-advisor",
                "matrix-tester"
            ]
        );
        assert_eq!(plan.files, [("greet.cjs".to_owned(), "a".to_owned())]);
        assert_eq!(plan.brief, BRIEF);
    }
}
