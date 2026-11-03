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
use crate::live::members::{Members, Plan};
use crate::live::report::Check;
use crate::live::Harness;
use crate::{Error, Result};

/// What the project's folder tells every window (`AGENTS.md`, `CLAUDE.md`): the kit's own brief
/// has a window run no tool that a message does not name, and a worker asked to change a file
/// needs its tools to do it.
pub const BRIEF: &str = "This folder is an automated ConsensFlow test. Do what each message asks, and nothing more: use your tools and run commands where the work it asks for needs them, change no file it does not ask you to change, and keep your reply short.\n";

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

    /// The position called `name`, as [`Role::name`] says it.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.name() == name)
    }

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

/// The harnesses in the order the Latin square turns them: board `k` seats in position `r`
/// (in [`Role::ALL`]'s order) the harness `k + r` places on, so each harness sits in each
/// position on exactly one of the five boards (LMX-03's table; LMX-05).
pub const ORDER: [Harness; 5] = [
    Harness::Claude,
    Harness::Codex,
    Harness::Pi,
    Harness::Devin,
    Harness::Muse,
];

/// How many boards the Latin square has.
pub const BOARDS: u8 = 5;

/// The platforms each board runs on.
pub const PLATFORMS: [&str; 2] = ["macos", "windows"];

/// The harness of each position of board `board` (0 to 4).
pub fn row_of(board: u8) -> [(Role, Harness); 5] {
    Role::ALL.map(|role| {
        let at = Role::ALL
            .iter()
            .position(|each| *each == role)
            .unwrap_or_default();
        (role, ORDER[(usize::from(board) + at) % ORDER.len()])
    })
}

/// One cell of the matrix: a harness in a position, on a platform, and the board that seats it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub platform: &'static str,
    pub harness: Harness,
    pub role: Role,
    pub board: u8,
}

/// Every cell the five boards cover on both platforms: 50, each harness in each position once
/// a platform.
pub fn cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for platform in PLATFORMS {
        for board in 0..BOARDS {
            for (role, harness) in row_of(board) {
                cells.push(Cell {
                    platform,
                    harness,
                    role,
                    board,
                });
            }
        }
    }
    let place = |harness: Harness| ORDER.iter().position(|each| *each == harness);
    cells.sort_by_key(|cell| (cell.platform, place(cell.harness), cell.role, cell.board));
    cells
}

/// The plan a person reads: the five boards, then every cell with the board that covers it.
pub fn plan_text() -> String {
    let mut text = String::from("The Latin square: board, then the harness of each position.\n");
    text.push_str("board  chief   worker  reviewer advisor tester\n");
    for board in 0..BOARDS {
        let row: Vec<String> = row_of(board)
            .iter()
            .map(|(_, harness)| format!("{:<8}", harness.name()))
            .collect();
        text.push_str(&format!("{board:<6} {}\n", row.join("").trim_end()));
    }
    let cells = cells();
    text.push_str(&format!(
        "\n{} cells (harness x position x platform), each covered by one board:\n",
        cells.len()
    ));
    for cell in &cells {
        text.push_str(&format!(
            "{:<8} {:<7} {:<9} board {}\n",
            cell.platform,
            cell.harness.name(),
            cell.role.name(),
            cell.board
        ));
    }
    text.push_str("\nThe image designer is a sixth member and Codex alone (`--seats ...,designer=codex`): the catalog refuses `designer: true` for every other harness.\n");
    text
}

/// The roster row of a stand-in member besides a board's five: the agent `agent` at `tier`.
pub fn stand_in_row(agent: &str, tier: &str) -> Value {
    json!({ "id": agent, "kind": "claude-code", "model": STAND_IN_WORKER_MODEL, "workTier": tier })
}

/// The five positions of a board and who sits in each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seating {
    /// The board's number, where the seats are one of the Latin square's rows.
    pub board: Option<u8>,
    pub seats: Vec<Seat>,
    /// Whether every position is the rig's stand-in's: the control, which covers nothing.
    pub control: bool,
    /// The image designer, a sixth member, where the run seats one: Codex alone.
    pub designer: Option<Harness>,
}

impl Seating {
    /// Board `number` of the Latin square, 0 to 4 ([`row_of`]).
    pub fn board(number: u8) -> Result<Self> {
        if number >= BOARDS {
            return Err(Error::Daemon(format!(
                "the Latin square has boards 0 to {}: no board {number}",
                BOARDS - 1
            )));
        }
        Ok(Self::real(Some(number), row_of(number).to_vec(), None))
    }

    /// The seating of `seats`, each a real harness, with the board it is where it is one.
    fn real(board: Option<u8>, seats: Vec<(Role, Harness)>, designer: Option<Harness>) -> Self {
        Self {
            board,
            seats: seats
                .into_iter()
                .map(|(role, harness)| Seat {
                    role,
                    harness: Some(harness),
                })
                .collect(),
            control: false,
            designer,
        }
    }

    /// The seats `words` name (`--seats chief=claude,worker=codex,…`): every one of the five
    /// positions once, each a harness, and the image designer besides where it is named.
    /// Refused in words: a position or a harness that is none, one named twice or not at all,
    /// and a designer that is not Codex, or that sits on a board with no Codex to run it.
    pub fn named(words: &str) -> Result<Self> {
        let refuse = |why: String| Err(Error::Daemon(format!("--seats {words}: {why}")));
        let mut seats: Vec<(Role, Harness)> = Vec::new();
        let mut designer = None;
        for piece in words
            .split(',')
            .map(str::trim)
            .filter(|piece| !piece.is_empty())
        {
            let Some((position, harness)) = piece.split_once('=') else {
                return refuse(format!("{piece} is no position=harness"));
            };
            let (position, name) = (position.trim(), harness.trim());
            let Some(harness) = Harness::from_name(name) else {
                return refuse(format!(
                    "no harness is called {name} ({})",
                    ORDER.map(Harness::name).join(", ")
                ));
            };
            if position == "designer" {
                if harness != Harness::Codex {
                    return refuse(format!(
                        "only Codex can be the image designer (the catalog refuses `designer: true` for {}), and it sits as a sixth member",
                        harness.name()
                    ));
                }
                designer = Some(harness);
                continue;
            }
            let Some(role) = Role::named(position) else {
                return refuse(format!(
                    "no position is called {position} (chief, worker, reviewer, advisor, tester, or designer for the sixth member)"
                ));
            };
            if seats.iter().any(|(seated, _)| *seated == role) {
                return refuse(format!("{position} is named twice"));
            }
            seats.push((role, harness));
        }
        let missing: Vec<&str> = Role::ALL
            .iter()
            .filter(|role| !seats.iter().any(|(seated, _)| seated == *role))
            .map(|role| role.name())
            .collect();
        if !missing.is_empty() {
            return refuse(format!(
                "every position needs a harness, and {} has none",
                missing.join(", ")
            ));
        }
        if designer.is_some() && !seats.iter().any(|(_, harness)| *harness == Harness::Codex) {
            return refuse(
                "the image designer runs on Codex, and no position of the board seats Codex for the rig to find it"
                    .to_owned(),
            );
        }
        seats.sort_by_key(|(role, _)| *role);
        let board = (0..BOARDS).find(|board| row_of(*board).as_slice() == seats.as_slice());
        Ok(Self::real(
            board.filter(|_| designer.is_none()),
            seats,
            designer,
        ))
    }

    /// The seating a run's manifest says it had ([`Seating::manifest`]): what a replay seats.
    pub fn from_manifest(manifest: &Value) -> Result<Self> {
        let refuse =
            |why: &str| Error::Daemon(format!("the manifest's seats cannot be read: {why}"));
        let seats = manifest["seats"]
            .as_array()
            .ok_or_else(|| refuse("it has no seats"))?
            .iter()
            .map(|seat| {
                let role = seat["role"]
                    .as_str()
                    .and_then(Role::named)
                    .ok_or_else(|| refuse("a seat has no position"))?;
                let harness = match seat["harness"].as_str() {
                    Some("stand-in") => None,
                    Some(name) => Some(
                        Harness::from_name(name)
                            .ok_or_else(|| refuse(&format!("no harness is called {name}")))?,
                    ),
                    None => return Err(refuse("a seat has no harness")),
                };
                Ok(Seat { role, harness })
            })
            .collect::<Result<Vec<_>>>()?;
        if Role::ALL
            .iter()
            .any(|role| seats.iter().filter(|seat| seat.role == *role).count() != 1)
        {
            return Err(refuse("it does not seat each position once"));
        }
        let designer = match manifest["designer"].as_str() {
            Some("codex") => Some(Harness::Codex),
            Some(other) => {
                return Err(refuse(&format!(
                    "its designer is {other}, and only Codex can be"
                )))
            }
            None => None,
        };
        Ok(Self {
            board: manifest["board"]
                .as_u64()
                .and_then(|board| u8::try_from(board).ok()),
            seats,
            control: manifest["control"] == true,
            designer,
        })
    }

    /// The five positions, each played by the rig's stand-in.
    pub fn stand_ins() -> Self {
        Self {
            board: None,
            control: true,
            designer: None,
            seats: Role::ALL
                .iter()
                .map(|role| Seat {
                    role: *role,
                    harness: None,
                })
                .collect(),
        }
    }

    /// The same seating with the members of `roles` played by the rig's stand-in: a seat whose
    /// harness cannot be real on this machine today (Pi with no login of its model's
    /// provider), which the run says it does not cover. The chief is the board's own harness
    /// or nothing: a stand-in chief is the control's, told what to do in its own words.
    pub fn with_stand_ins(mut self, roles: &[Role]) -> Result<Self> {
        if roles.contains(&Role::Chief) {
            return Err(Error::Daemon(
                "--stand-in takes a member's position (worker, reviewer, advisor, tester), not the chief's"
                    .to_owned(),
            ));
        }
        for seat in &mut self.seats {
            if roles.contains(&seat.role) {
                seat.harness = None;
            }
        }
        Ok(self)
    }

    /// The positions the stand-in plays where the board seats a real harness: what the run
    /// covers none of.
    pub fn not_covered(&self) -> Vec<Role> {
        if self.control {
            return Vec::new();
        }
        self.seats
            .iter()
            .filter(|seat| seat.harness.is_none())
            .map(|seat| seat.role)
            .collect()
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

    /// The project the board opens: the folder `folder`, told [`BRIEF`], the files
    /// `files`, and the roster of the five seats, the chief first.
    pub fn plan(&self, folder: &str, files: &[(&str, &str)]) -> Plan {
        let mut plan = Plan::new(folder, self.seat(Role::Chief).row()).told(BRIEF);
        for seat in self.seats.iter().filter(|seat| seat.role != Role::Chief) {
            plan = plan.with(seat.row());
        }
        for (path, text) in files {
            plan = plan.with_file(path, *text);
        }
        // The daemon writes what each window asked of the API and how it was answered, the
        // refusals' codes among it, for the oracle (`cf_daemon::api::calls`): its own alone.
        plan.with_daemon_var(super::observe::TOOLING_API_CALLS, "1")
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
        if self.designer.is_some() {
            // The catalog's own image agent, beside the five: it fills no other position.
            let what = "the image designer codex is staff".to_owned();
            checks.push(match members.add(agents::DESIGNER, &["designer"]) {
                Ok(tier) => Check::passed(
                    "staff",
                    &what,
                    format!("member.add gave {} the tier {tier:?}", agents::DESIGNER),
                ),
                Err(failed) => Check::failed("staff", &what, failed.to_string()),
            });
        }
        checks
    }

    /// What it is, as a run id says it: `board3`, `seats` or `stand-ins`.
    pub fn label(&self) -> String {
        match (self.control, self.board) {
            (true, _) => "stand-ins".to_owned(),
            (false, Some(board)) => format!("board{board}"),
            (false, None) => "seats".to_owned(),
        }
    }

    /// The seating, as the evidence's manifest has it and a replay reads it back
    /// ([`Seating::from_manifest`]).
    pub fn manifest(&self) -> Value {
        json!({
            "board": self.board,
            "control": self.control,
            "designer": self.designer.map(Harness::name),
            "seats": self
                .seats
                .iter()
                .map(|seat| {
                    json!({
                        "role": seat.role.name(),
                        "harness": seat.who(),
                        "agent": seat.agent(),
                        "row": seat.row(),
                    })
                })
                .collect::<Vec<_>>(),
        })
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
        assert_eq!(row_of(0), BOARD_0);
        let refused = Seating::board(5).unwrap_err().to_string();
        assert!(refused.contains("boards 0 to 4: no board 5"), "{refused}");
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
    fn a_member_the_machine_cannot_seat_today_is_played_by_the_stand_in_and_is_not_covered() {
        let board = Seating::board(0)
            .unwrap()
            .with_stand_ins(&[Role::Reviewer])
            .unwrap();
        assert_eq!(board.seat(Role::Reviewer).who(), "stand-in");
        assert_eq!(
            board.seat(Role::Reviewer).row()["model"],
            STAND_IN_WORKER_MODEL
        );
        assert_eq!(board.seat(Role::Chief).who(), "claude");
        assert!(!board.chief_is_stand_in());
        assert_eq!(board.not_covered(), [Role::Reviewer]);
        assert!(Seating::board(0).unwrap().not_covered().is_empty());
        // The control covers nothing and claims nothing.
        assert!(Seating::stand_ins().not_covered().is_empty());
        let refused = Seating::board(0)
            .unwrap()
            .with_stand_ins(&[Role::Chief])
            .unwrap_err()
            .to_string();
        assert!(refused.contains("not the chief's"), "{refused}");
        assert_eq!(Role::named("tester"), Some(Role::Tester));
        assert_eq!(Role::named("designer"), None);
    }

    #[test]
    fn the_five_boards_are_the_designs_table_and_each_harness_sits_in_each_position_once() {
        let names = |board: u8| row_of(board).map(|(_, harness)| harness.name());
        assert_eq!(names(0), ["claude", "codex", "pi", "devin", "muse"]);
        assert_eq!(names(1), ["codex", "pi", "devin", "muse", "claude"]);
        assert_eq!(names(2), ["pi", "devin", "muse", "claude", "codex"]);
        assert_eq!(names(3), ["devin", "muse", "claude", "codex", "pi"]);
        assert_eq!(names(4), ["muse", "claude", "codex", "pi", "devin"]);
        for board in 0..BOARDS {
            let seating = Seating::board(board).unwrap();
            assert_eq!(
                (seating.board, seating.control, seating.designer),
                (Some(board), false, None)
            );
            assert_eq!(seating.label(), format!("board{board}"));
        }
    }

    #[test]
    fn the_plan_lists_fifty_cells_each_covered_once() {
        let cells = cells();
        assert_eq!(cells.len(), 50);
        for platform in PLATFORMS {
            for harness in ORDER {
                for role in Role::ALL {
                    let covering: Vec<&Cell> = cells
                        .iter()
                        .filter(|cell| {
                            cell.platform == platform
                                && cell.harness == harness
                                && cell.role == role
                        })
                        .collect();
                    assert_eq!(
                        covering.len(),
                        1,
                        "{platform} {} {}",
                        harness.name(),
                        role.name()
                    );
                }
            }
        }
        let text = plan_text();
        assert!(text.contains("50 cells"), "{text}");
        assert!(
            text.contains("windows  muse    chief     board 4"),
            "{text}"
        );
        assert!(
            text.contains("2      pi      devin   muse    claude  codex"),
            "{text}"
        );
    }

    #[test]
    fn seats_named_are_every_position_once_and_a_row_of_the_square_is_its_board() {
        let seats =
            Seating::named("tester=claude,chief=codex,worker=pi,reviewer=devin,advisor=muse")
                .unwrap();
        assert_eq!(seats.board, Some(1));
        let mixed =
            Seating::named("chief=claude,worker=devin,reviewer=codex,advisor=pi,tester=muse")
                .unwrap();
        assert_eq!((mixed.board, mixed.label().as_str()), (None, "seats"));
        assert_eq!(mixed.seat(Role::Worker).who(), "devin");
        // Two of one harness are two agents of their own.
        let twice = Seating::named("chief=claude,worker=claude,reviewer=pi,advisor=pi,tester=muse")
            .unwrap();
        assert_eq!(twice.seat(Role::Worker).agent(), "matrix-worker");
        let designer = Seating::named(
            "chief=claude,worker=codex,reviewer=pi,advisor=devin,tester=muse,designer=codex",
        )
        .unwrap();
        assert_eq!(
            (designer.designer, designer.board),
            (Some(Harness::Codex), None)
        );
    }

    #[test]
    fn unsupported_seats_are_refused_in_words() {
        let refused = |words: &str| Seating::named(words).unwrap_err().to_string();
        let five = "chief=claude,worker=codex,reviewer=pi,advisor=devin,tester=muse";
        for designer in ["claude", "pi", "devin", "muse"] {
            let said = refused(&format!("{five},designer={designer}"));
            assert!(
                said.contains("only Codex can be the image designer"),
                "{said}"
            );
            assert!(
                said.contains(&format!("refuses `designer: true` for {designer}")),
                "{said}"
            );
        }
        assert!(refused(
            "chief=claude,worker=claude,reviewer=pi,advisor=devin,tester=muse,designer=codex"
        )
        .contains("no position of the board seats Codex"));
        assert!(refused("chief=claude,worker=codex").contains("reviewer, advisor, tester has none"));
        assert!(refused(&format!("{five},chief=pi")).contains("chief is named twice"));
        assert!(refused("chief=kimi").contains("no harness is called kimi"));
        assert!(refused("boss=claude").contains("no position is called boss"));
        assert!(refused("claude").contains("claude is no position=harness"));
    }

    #[test]
    fn a_seating_is_read_back_from_its_manifest_as_it_was() {
        for seating in [
            Seating::board(3).unwrap(),
            Seating::board(0)
                .unwrap()
                .with_stand_ins(&[Role::Reviewer])
                .unwrap(),
            Seating::stand_ins(),
            Seating::named(
                "chief=claude,worker=codex,reviewer=pi,advisor=devin,tester=muse,designer=codex",
            )
            .unwrap(),
        ] {
            let manifest = seating.manifest();
            assert_eq!(
                Seating::from_manifest(&manifest).unwrap(),
                seating,
                "{manifest}"
            );
        }
        let mut broken = Seating::board(0).unwrap().manifest();
        broken["seats"].as_array_mut().unwrap().pop();
        assert!(Seating::from_manifest(&broken)
            .unwrap_err()
            .to_string()
            .contains("each position once"));
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
        assert!(
            BRIEF.contains("use your tools and run commands where the work it asks for needs them")
        );
    }
}
