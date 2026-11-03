"""Moves the goldens a member named by its model changes (spec harness-models).

A member is a harness, a model, an effort and a tier: no saved agent behind it, and the
staff table of a chief's text says the model each member runs on, in a column of its own
after the member (`| Member | Model | Roles | Work tier |`).

    python3 -I tools/move_harness_models_goldens.py text crates/cf-engine/tests/goldens/text.json

`text` moves the role texts' tables: each staff row of `teamTable` and `roleInstructions`
that says no model runs on `Opus · high` (what the text tests' rows run on where a row says
none, `MODEL_SHOWN` in `crates/cf-engine/tests/text/support.rs`), and the table each
expected text holds gains that column; each `staffOf` row's members say what the
participants they are read off run on. A row already moved says its model and is left as
it is, so the move can be run again after a merge brings unmoved rows.
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


def main(argv):
    if len(argv) != 3 or argv[1] != "text":
        sys.exit(__doc__)
    move_text(argv[2])


if __name__ == "__main__":
    main(sys.argv)
