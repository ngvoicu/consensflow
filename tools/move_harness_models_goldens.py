"""Moves the goldens a member named by its model changes (spec harness-models).

A member is a harness, a model, an effort and a tier: no saved agent behind it, and the
staff table of a chief's text says the model each member runs on, in a column of its own
after the member (`| Member | Model | Roles | Work tier |`).

    python3 -I tools/move_harness_models_goldens.py text crates/cf-engine/tests/goldens/text.json
    python3 -I tools/move_harness_models_goldens.py cli crates/cf/tests/goldens/cli.<platform>.json
    python3 -I tools/move_harness_models_goldens.py board crates/cf/tests/goldens/board.json

`text` moves the role texts' tables: each staff row of `teamTable` and `roleInstructions`
that says no model runs on `Opus · high` (what the text tests' rows run on where a row says
none, `MODEL_SHOWN` in `crates/cf-engine/tests/text/support.rs`), and the table each
expected text holds gains that column; each `staffOf` row's members say what the
participants they are read off run on. A row already moved says its model and is left as
it is, so the move can be run again after a merge brings unmoved rows.

`cli` moves `cf`'s recordings of its standalone verbs: the cases of `catalog` and of
`agent` (its catalog and the human's saved agents, which are no more) go; every usage
printed loses their lines, and `ui` and `doctor` their words for them; `setup` and
`doctor` count no agents (the line `agents: …` goes), and their two cases that stopped
at a file of agents they could not read go, as neither reads one. What else a case printed, wrote
and exited with is as Node's was, and the rest of `cli_goldens` holds it.

`board` moves `cf staff`'s two cases: the board answers each member with what it runs on
(`/api/staff`: its harness and the harness's name, the model's id and name, its effort),
and `cf staff` prints it after the handle (`@opus-high · Opus · high (Claude Code,
claude-opus-5-5) · worker · standard`); with no member it says members, not agents. A
case already moved is left as it is.
"""

import json
import sys

MODEL_SHOWN = "Opus · high"

# What `cf_ledger::handles::harness_label` calls a harness.
HARNESS_LABEL = {
    "claude-code": "Claude Code",
    "codex": "Codex",
    "pi": "Pi",
    "devin": "Devin",
    "muse": "Muse Code",
}

TIERS = {
    "critical": "Critical work",
    "complex": "Complex work",
    "standard": "Standard work",
    "light": "Light work",
}


def cell(value):
    """`roles::skill::cell`: a bar escaped, a run of line ends one space."""
    out = []
    after_break = False
    for character in value:
        if character == "|":
            out.append("\\|")
        elif character in "\r\n":
            if not after_break:
                out.append(" ")
        else:
            out.append(character)
        after_break = character in "\r\n"
    return "".join(out)


def table(members, with_model):
    """The staff table as `roles::skill::team_table` writes it; none where a member has no tier."""
    if with_model:
        lines = ["| Member | Model | Roles | Work tier |", "|---|---|---|---|"]
    else:
        lines = ["| Member | Roles | Work tier |", "|---|---|---|"]
    for member in members:
        tier = TIERS.get(member.get("workTier"))
        if tier is None:
            return None
        cells = [member["name"]]
        if with_model:
            cells.append(member.get("model", MODEL_SHOWN))
        cells += [", ".join(member["roles"]), tier]
        lines.append("| " + " | ".join(cell(text) for text in cells) + " |")
    return "\n".join(lines)


def shown(participant):
    """`cf_ledger::handles::model_and_effort` of a participant row."""
    name = participant.get("modelName")
    if not (name and name.strip()):
        name = participant.get("model")
    if not name:
        harness = participant.get("harness")
        name = "%s default" % HARNESS_LABEL.get(harness, harness or "its harness's")
    effort = participant.get("effort")
    return "%s · %s" % (name, effort) if effort else name


def move_text(path):
    with open(path, encoding="utf-8") as file:
        golden = json.load(file)
    moved = 0
    for row in golden["teamTable"]:
        if "table" in row and row["table"].startswith("| Member | Roles |"):
            row["table"] = table(row["members"], True)
            moved += 1
    for row in golden["roleInstructions"]:
        text = row["answer"].get("text") if isinstance(row["answer"], dict) else None
        if row["role"] != "chief" or not text or not row["staff"]:
            continue
        before = table(row["staff"], False)
        after = table(row["staff"], True)
        if before is not None and before in text:
            row["answer"]["text"] = text.replace(before, after, 1)
            moved += 1
    for row in golden["staffOf"]:
        by_handle = {participant["handle"]: participant for participant in row["participants"]}
        for member in row["staff"]:
            if "model" not in member:
                member["model"] = shown(by_handle[member["name"]])
                moved += 1
    with open(path, "w", encoding="utf-8") as file:
        file.write(json.dumps(golden, indent=2, ensure_ascii=False) + "\n")
    print("moved %d rows of %s" % (moved, path))


OLD_USAGE = """  catalog [--harness <h>] [--json]            List available agent presets
  agent add <name>                           Add an agent of your own
    [--harness <h>] [--model <m>] [--effort <e>] [--description <d>]
    [--designer]                            An image agent (Codex only): an image designer
  agent list [--json]
  agent edit <name> [--model <m>] [--effort <e>] [--description <d>]
    [--work-tier critical|complex|standard|light|auto]
  agent remove <name>
"""

WORDS = [
    (
        "Run the app's daemon; open the agents screens",
        "Run the app's daemon; open the harnesses screen",
    ),
    (
        "Inspect runtime, roster and bundled roles",
        "Inspect runtime, harnesses and bundled roles",
    ),
]


def moved_text(text):
    """A case's print with the usage and the counts moved."""
    if text is None:
        return None
    text = text.replace(OLD_USAGE, "")
    for before, after in WORDS:
        text = text.replace(before, after)
    lines = text.split("\n")
    lines = [
        line
        for line in lines
        if not (line.startswith("agents: ") and " saved — manage them with cf ui or cf agent" in line)
        and not line.startswith("agents:       ")
    ]
    return "\n".join(lines)


# The cases of `setup` and `doctor` that stopped at a file of saved agents they could not
# read: neither reads one now, so neither stops.
UNREAD = {
    "setup: an agents file it cannot read, after what it said so far",
    "doctor: an agents file it cannot read, after what it said so far",
}


def move_cli(path):
    with open(path, encoding="utf-8") as file:
        golden = json.load(file)
    before = len(golden["cases"])
    golden["cases"] = [
        case
        for case in golden["cases"]
        if (not case["args"] or case["args"][0] not in ("catalog", "agent"))
        and case["name"] not in UNREAD
    ]
    for case in golden["cases"]:
        for key in ("stdout", "stderr"):
            case[key] = moved_text(case.get(key))
        rust = case.get("kept", {}).get("rust", {})
        for key in ("stdout", "stderr"):
            if key in rust:
                rust[key] = moved_text(rust[key])
    with open(path, "w", encoding="utf-8") as file:
        file.write(cli_text(golden))
    print("%s: %d cases, %d went" % (path, len(golden["cases"]), before - len(golden["cases"])))


def cli_text(golden):
    """The recording as it is written: its head two spaces in, each case on a line of its own."""
    head = ['  "%s": %s' % (key, json.dumps(value, ensure_ascii=False)) for key, value in golden.items() if key != "cases"]
    cases = ",\n".join(
        "    " + json.dumps(case, ensure_ascii=False, separators=(",", ":")) for case in golden["cases"]
    )
    return "{\n" + ",\n".join(head) + ',\n  "cases": [\n' + cases + "\n  ]\n}\n"


STAFF = [
    {
        "handle": "opus-high",
        "role": "worker",
        "roles": ["worker"],
        "tier": "standard",
        "harness": "claude-code",
        "harnessName": "Claude Code",
        "model": "claude-opus-5-5",
        "modelName": "Opus",
        "effort": "high",
    },
    {
        "handle": "codex-default",
        "role": "advisor",
        "roles": ["advisor", "reviewer"],
        "tier": "critical",
        "harness": "codex",
        "harnessName": "Codex",
        "model": None,
        "modelName": "Codex default",
        "effort": None,
    },
]

STAFF_SAID = (
    "@opus-high · Opus · high (Claude Code, claude-opus-5-5) · worker · standard\n"
    "@codex-default · Codex default (Codex, its default model) · advisor+reviewer · critical\n"
)

NO_STAFF_SAID = "No members are on this project's staff yet; the human adds them in the app.\n"


def move_board(path):
    with open(path, encoding="utf-8") as file:
        golden = json.load(file)
    moved = 0
    for case in golden:
        if case["name"] == "no staff" and case["stdout"] != NO_STAFF_SAID:
            case["stdout"] = NO_STAFF_SAID
            moved += 1
        if case["name"] == "the staff" and case["stdout"] != STAFF_SAID:
            case["replies"][0]["text"] = json.dumps(
                {"members": STAFF}, ensure_ascii=False, separators=(",", ":")
            )
            case["stdout"] = STAFF_SAID
            moved += 1
    with open(path, "w", encoding="utf-8") as file:
        file.write(json.dumps(golden, indent=2, ensure_ascii=False) + "\n")
    print("moved %d cases of %s" % (moved, path))


def main(argv):
    if len(argv) != 3 or argv[1] not in ("text", "cli", "board"):
        sys.exit(__doc__)
    {"text": move_text, "cli": move_cli, "board": move_board}[argv[1]](argv[2])


if __name__ == "__main__":
    main(sys.argv)
