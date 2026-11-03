"""Moves Node's recordings to a chief that is a harness and a model (CHF-09, T-729).

Node's chief ran on a saved agent: its requests named one (`chief: {harness, agent}`,
`switchChief(project, {harness, agent})`), its participant said it (`agent`), its switches
were logged with one on each side (`chief.switched {from: {harness, agent}, to: …}`), its
handoff named it (`Codex (astraeus)`), and its launch was given that agent's saved model.
A chief now runs on a harness and the model picked for it (CHF-04): it names no saved
agent (`agent` null), its participant says the model after its harness, its switches name
a model on each side, its handoff the model, and its launch is given that model with no
effort, thinking or designer of a saved agent's.

This changes exactly those, and nothing else, in each recording given:

  python3 -I tools/move_chief_model_goldens.py engine <trace.json.gz>…
  python3 -I tools/move_chief_model_goldens.py ledger <trace.json.gz>…
  python3 -I tools/move_chief_model_goldens.py daemon <trace.json.gz>…

`engine` is a trace of Node's dispatcher (`crates/cf-engine/tests/traces`, and the
engine's own departures beside it): a chief's model is its agent's saved model in the
kit's roster (`MODELS` in `crates/cf-engine/src/testing/seams.rs`), what Node launched it
on, else the agent's name. `ledger` is a trace of Node's ledger
(`crates/cf-ledger/tests/traces`): no roster is there, and a chief's model is its agent's
name. `daemon` is a trace of Node's daemon (`crates/cf-daemon/tests/goldens`, each surface):
a request names the chief's agent, which the trace's own roster (its world's
`agents.json`, then the catalog's ready-made agents, `crates/cf-catalog/data/presets.json`)
says the harness and model of; one that names none the roster has is
refused for naming no model. A recording already moved names no agent of a chief, and is left as it is, so the
move can be run again. It prints, for each family, how many of each change it made, and
the files it wrote.
"""

import gzip
import json
import os
import re
import sys

HARNESS_NAMES = {"claude-code": "Claude Code", "codex": "Codex", "pi": "Pi", "devin": "Devin"}
HERE = os.path.dirname(os.path.abspath(__file__))
SEAMS = os.path.join(HERE, "..", "crates", "cf-engine", "src", "testing", "seams.rs")
PRESETS = os.path.join(HERE, "..", "crates", "cf-catalog", "data", "presets.json")


def kit_models():
    """The kit roster's saved model of each fake agent."""
    text = open(SEAMS, encoding="utf-8").read()
    table = text[text.index("pub const MODELS"):]
    table = table[: table.index("];")]
    return dict(re.findall(r'\("([^"]+)", "([^"]+)"\)', table))


class Mover:
    def __init__(self, model_of):
        self.model_of = model_of
        self.counts = {}
        # The projects whose chief is one this build made or switched: none said, every one.
        self.only = None
        # The switches this build logged, by project and time: none said, every one.
        self.logged = None
        # The project a `lastSwitch` asked of.
        self.project = None

    def count(self, what):
        self.counts[what] = self.counts.get(what, 0) + 1

    def model(self, agent):
        return None if agent is None else self.model_of(agent)

    # Requests ---------------------------------------------------------------

    def request_chief(self, chief):
        """`{harness, agent}` as a request names a chief: `{harness, model}`."""
        if not isinstance(chief, dict) or "agent" not in chief or "role" in chief:
            return chief
        self.count("request chief")
        moved = {}
        for key, value in chief.items():
            if key == "agent":
                if value is not None:
                    moved["model"] = self.model(value)
            else:
                moved[key] = value
        return moved

    def switch_request(self, asked):
        """`{harness, agent, …}` as a Switch chief is asked: the model in the agent's place."""
        if not isinstance(asked, dict) or "agent" not in asked or "harness" not in asked:
            return asked
        self.count("switch request")
        return {("model" if key == "agent" else key): (self.model(value) if key == "agent" else value)
                for key, value in asked.items()}

    # Answers and events -------------------------------------------------------

    def value(self, value):
        """Every chief participant, switch and handoff text `value` holds, moved."""
        if isinstance(value, list):
            return [self.value(item) for item in value]
        if isinstance(value, str):
            return self.text(value)
        if not isinstance(value, dict):
            return value
        if value.get("role") == "chief" and value.get("handle") == "chief" \
                and value.get("agent") is not None and "harness" in value and "model" not in value:
            return self.participant(value)
        if value.get("kind") == "chief.switched" and isinstance(value.get("data"), dict) \
                and (self.logged is None or (value.get("project", value.get("projectId")), value.get("at")) in self.logged):
            moved = dict(value)
            moved["data"] = self.switched(value["data"])
            return {key: self.value(item) if key != "data" else item for key, item in moved.items()}
        if isinstance(value.get("switching"), dict) and "agent" in value["switching"]:
            self.count("lane switching")
            moved = {key: self.value(item) for key, item in value.items() if key != "switching"}
            return {key: (self.side(value["switching"]) if key == "switching" else moved[key])
                    for key in value}
        if set(value) == {"from", "cut"} and isinstance(value["from"], dict) and "agent" in value["from"]:
            self.count("last switch")
            if self.only is not None and self.project not in self.only:
                # A switch an older build made: what it was from names no model.
                side = {("model" if key == "agent" else key): (None if key == "agent" else item)
                        for key, item in value["from"].items()}
                return {"from": side, "cut": value["cut"]}
            return {"from": self.side(value["from"]), "cut": value["cut"]}
        return {key: self.value(item) for key, item in value.items()}

    def participant(self, view):
        if self.only is not None and view.get("projectId") not in self.only:
            return {key: self.value(item) for key, item in view.items()}
        self.count("chief participant")
        agent = view["agent"]
        moved = {}
        for key, item in view.items():
            moved[key] = None if key == "agent" else self.value(item)
            if key == "harness" and agent is not None:
                moved["model"] = self.model(agent)
        return moved

    def side(self, side):
        return {("model" if key == "agent" else key): (self.model(item) if key == "agent" else item)
                for key, item in side.items()}

    def switched(self, data):
        if not any(isinstance(data.get(end), dict) and "agent" in data[end] for end in ("from", "to")):
            return data
        self.count("chief.switched event")
        return {key: self.side(item) if key in ("from", "to") and isinstance(item, dict) else item
                for key, item in data.items()}

    def sides(self, match):
        """One sentence of a handoff or of a switch's note, each side's name the model."""
        named = lambda name: "" if name is None else f" ({self.model(name)})"
        return f"{match.group(1)}{named(match.group(2))}{match.group(3)}{named(match.group(4))}"

    def asked(self, match):
        return f"{match.group(1)} ({self.model(match.group(2))})"

    def text(self, text):
        """A handoff's and a switch note's names of the chief, each the model for the agent,
        in a text as JSON holds it and as SQL quotes it (`project''s`). A handoff from
        before 2026-10-03 (to "the lead") was written then and stays as it was."""
        names = "|".join(re.escape(name) for name in HARNESS_NAMES.values())
        name = r"(?: \(([A-Za-z0-9._-]+)\))?"
        moved = re.sub(
            r"(switched this project'{1,2}s chief from (?:" + names + r"))" + name
            + r"( to you, (?:" + names + r"))" + name,
            self.sides, text)
        moved = re.sub(r"(moving this project'{1,2}s chief to [a-z-]+) \(([A-Za-z0-9._-]+)\)", self.asked, moved)
        if moved != text:
            self.count("handoff or note text")
        return moved

    # The database -------------------------------------------------------------

    def tables(self, tables):
        """The participant table's chief rows name no agent; the switches' data and the
        texts as above."""
        participant = tables.get("participant")
        if isinstance(participant, dict):
            columns = participant["columns"]
            role = columns.index("role")
            # A chief names no agent, nor did the switch it was switched from.
            named = [columns.index(name) for name in ("agent", "switched_from_agent") if name in columns]
            project = columns.index("project_id")
            for row in participant["rows"]:
                if self.only is not None and int(row[project]) not in self.only:
                    continue
                if row[role] == "'chief'" and any(row[at] != "NULL" for at in named):
                    self.count("chief row")
                    for at in named:
                        row[at] = "NULL"
        event = tables.get("event")
        if isinstance(event, dict):
            columns = event["columns"]
            kind, data = columns.index("kind"), columns.index("data")
            project, at = columns.index("project_id"), columns.index("at")
            for row in event["rows"]:
                if self.logged is not None and (int(row[project]), row[at][1:-1]) not in self.logged:
                    continue
                if row[kind] == "'chief.switched'" and row[data].startswith("'"):
                    parsed = json.loads(row[data][1:-1].replace("''", "'"))
                    moved = self.switched(parsed)
                    if moved is not parsed:
                        row[data] = "'" + json.dumps(moved, separators=(",", ":"), ensure_ascii=False).replace("'", "''") + "'"
        for name, table in tables.items():
            if isinstance(table, dict) and "rows" in table:
                table["rows"] = [[self.text(cell) if isinstance(cell, str) else cell for cell in row]
                                 for row in table["rows"]]
        return tables


def move_engine(trace, mover):
    """A trace of Node's dispatcher."""
    chief_model = {}
    events = []
    for event in trace["events"]:
        event = dict(event)
        op = event.get("op")
        if op == "openProject" and event.get("args"):
            asked = dict(event["args"][0])
            if "chief" in asked:
                asked["chief"] = mover.request_chief(asked["chief"])
            event["args"] = [asked] + event["args"][1:]
        if op == "switchChief" and len(event.get("args", [])) > 1:
            event["args"] = [event["args"][0], mover.switch_request(event["args"][1])] + event["args"][2:]
        for key in ("answer", "event", "threw", "result"):
            if key in event:
                event[key] = mover.value(event[key])
        seam = event.get("seam", "")
        if seam == "ledger" and event.get("method") == "createProject" and event.get("args"):
            asked = dict(event["args"][0])
            if "chief" in asked:
                asked["chief"] = mover.request_chief(asked["chief"])
            event["args"] = [asked] + event["args"][1:]
        elif seam == "ledger" and event.get("method") == "switchChief" and len(event.get("args", [])) > 1:
            event["args"] = [event["args"][0], mover.switch_request(event["args"][1])] + event["args"][2:]
        elif "args" in event and not (seam.startswith("adapter:") and event.get("method") == "prepare"):
            event["args"] = mover.value(event["args"])
        # Where the chief's model is known: as the ledger answers its participant, and as a
        # switch logs it (one that waited for the chief's turn launches before any answer).
        for view in chiefs_in(event.get("answer")):
            chief_model[view.get("projectId")] = view.get("model")
        logged = event.get("event")
        if isinstance(logged, dict) and logged.get("kind") == "chief.switched":
            chief_model[logged.get("project")] = logged["data"].get("to", {}).get("model")
        if seam.startswith("adapter:") and event.get("method") == "prepare":
            given = dict(event["args"][0])
            if given.get("participant", {}).get("handle") == "chief":
                project = given.get("project", {}).get("id")
                model = chief_model.get(project)
                launched = given.get("agent")
                # What a launch reads of a saved agent: its model, and no effort, thinking or
                # designer of a chief's.
                same = (launched is None) == (model is None) and (
                    launched is None or (launched.get("model") == model
                                         and not launched.get("designer")
                                         and launched.get("effort") in (None, {"$undefined": True})
                                         and launched.get("thinking") in (None, {"$undefined": True})))
                if not same:
                    mover.count("chief launch")
                    given["agent"] = None if model is None else {"model": model}
            given = {key: mover.value(item) if key != "agent" else item for key, item in given.items()}
            event["args"] = [given] + event["args"][1:]
        events.append(event)
    trace["events"] = events
    trace["finals"] = [mover.tables(final) if isinstance(final, dict) else final for final in trace["finals"]]
    return trace


def chiefs_in(value):
    if isinstance(value, list):
        for item in value:
            yield from chiefs_in(item)
    elif isinstance(value, dict):
        if value.get("role") == "chief" and value.get("handle") == "chief" and "harness" in value:
            yield value
        else:
            for item in value.values():
                yield from chiefs_in(item)


def move_ledger(trace, mover):
    """A trace of Node's ledger. A ledger an older build left keeps its chiefs as it holds
    them (an agent, no model: CHF-04), and the switches it logged: only the chief of a
    project the trace creates, or switches, moves, from that call on, and only the switches
    the trace logs. What a switch of an older build was from names no model."""
    mover.only = set()
    mover.logged = set()
    calls = []
    for call in trace["calls"]:
        call = dict(call)
        method = call.get("method")
        args = call.get("args", [])
        if method == "createProject" and args:
            asked = dict(args[0])
            if "chief" in asked:
                asked["chief"] = mover.request_chief(asked["chief"])
            call["args"] = [asked] + args[1:]
            if isinstance(call.get("result"), dict) and "id" in call["result"]:
                mover.only.add(call["result"]["id"])
        elif method == "switchChief" and len(args) > 1:
            call["args"] = [args[0], mover.switch_request(args[1])] + args[2:]
            if isinstance(call.get("result"), dict):
                mover.only.add(args[0])
        else:
            call["args"] = mover.value(args)
        for event in call.get("events", []):
            if isinstance(event, dict) and event.get("kind") == "chief.switched":
                mover.logged.add((event.get("project"), event.get("at")))
        # A chief's model that is no model's id is refused as one, where its agent was.
        refused = call.get("result", {}).get("$error") if isinstance(call.get("result"), dict) else None
        if method in ("createProject", "switchChief") and isinstance(refused, dict) \
                and refused.get("code") == "invalid-agent":
            mover.count("refusal of no model id")
            refused["code"] = "invalid-model"
            refused["message"] = refused["message"].replace("not an agent id:", "not a model id:")
        mover.project = args[0] if method == "lastSwitch" and args else None
        for key in ("result", "events", "callbacks", "error"):
            if key in call:
                call[key] = mover.value(call[key])
        calls.append(call)
    trace["calls"] = calls
    final = trace.get("final")
    if isinstance(final, dict) and isinstance(final.get("tables"), dict):
        mover.tables(final["tables"])
    mover.only = None
    mover.logged = None
    return trace


NO_AGENT = "pick one of your saved agents for the chief: its harness, model and effort come with it"
NO_MODEL = "pick a harness for the chief, and one of its models"


def move_page(trace, mover, roster):
    """A trace of Node's page (`crates/cf-daemon/tests/goldens`, surface `page`): a chief's
    request names the harness and the model its saved agent had in the trace's own roster
    (`consensflow/agents.json` of its world, then the catalog's ready-made agents) in the
    agent's place; one that named no agent the roster has, or none, is refused for naming
    no model."""
    for step in trace.get("steps", []):
        if step.get("kind") == "world":
            text = step.get("files", {}).get("consensflow/agents.json", {}).get("text")
            if text is not None:
                # A world may hold an agents file that is no JSON, on purpose: it saves none.
                try:
                    saved = json.loads(text).get("agents", [])
                except (ValueError, AttributeError):
                    saved = []
                roster.clear()
                roster.update({agent["id"]: agent for agent in saved
                               if isinstance(agent, dict) and isinstance(agent.get("id"), str)})
            continue
        if step.get("kind") == "ledger":
            # A call the trace makes of the ledger itself, as an operation's are made.
            moved = move_seam({"calls": [step]}, mover)["calls"][0]
            step.clear()
            step.update(moved)
            continue
        if step.get("kind") != "operation":
            if "result" in step:
                step["result"] = mover.value(step["result"])
            continue
        if step.get("name") in ("project.open", "chief.switch") and isinstance(step.get("body"), dict):
            body = step["body"]
            saved = mover.known(body.get("agent")) if isinstance(body.get("agent"), str) else None
            if "agent" in body and saved and saved.get("kind"):
                mover.count("page request")
                moved = {}
                for key, item in body.items():
                    if key == "agent":
                        moved["harness"] = saved["kind"]
                        moved["model"] = mover.model(item)
                    else:
                        moved[key] = item
                step["body"] = moved
            unknown = f"no agent named {body.get('agent')} in your agents"
            if isinstance(step.get("reply"), str) and (NO_AGENT in step["reply"] or unknown in step["reply"]):
                said = json.loads(step["reply"])
                if said.get("ok") is False:
                    mover.count("refusal for no model")
                    said["error"] = NO_MODEL
                    step["reply"] = json.dumps(said, separators=(",", ":"), ensure_ascii=False)
                    if isinstance(step.get("refusal"), dict):
                        step["refusal"]["message"] = NO_MODEL
        if isinstance(step.get("seams"), list):
            step["seams"] = [move_seam(seam, mover) for seam in step["seams"]]
        for key in ("events", "result"):
            if key in step:
                step[key] = mover.value(step[key])
        if isinstance(step.get("reply"), str):
            try:
                said = json.loads(step["reply"])
            except ValueError:
                continue
            moved = mover.value(said)
            if moved != said:
                step["reply"] = json.dumps(moved, separators=(",", ":"), ensure_ascii=False)
    final = (trace.get("ledger") or {}).get("final")
    if isinstance(final, dict) and isinstance(final.get("tables"), dict):
        mover.tables(final["tables"])
    return trace


def move_seam(seam, mover):
    """A call on the page's stand-in engine, and the ledger calls it made."""
    seam = dict(seam)
    args = seam.get("args", [])
    if seam.get("method") == "openProject" and args and isinstance(args[0], dict) and "chief" in args[0]:
        asked = dict(args[0])
        asked["chief"] = mover.request_chief(asked["chief"])
        seam["args"] = [asked] + args[1:]
    elif seam.get("method") == "switchChief" and len(args) > 1:
        seam["args"] = [args[0], mover.switch_request(args[1])] + args[2:]
    calls = []
    for call in seam.get("calls", []):
        call = dict(call)
        method, args = call.get("method"), call.get("args", [])
        if method == "createProject" and args and isinstance(args[0], dict) and "chief" in args[0]:
            asked = dict(args[0])
            asked["chief"] = mover.request_chief(asked["chief"])
            call["args"] = [asked] + args[1:]
        elif method == "switchChief" and len(args) > 1:
            call["args"] = [args[0], mover.switch_request(args[1])] + args[2:]
        for key in ("result", "events", "callbacks"):
            if key in call:
                call[key] = mover.value(call[key])
        calls.append(call)
    if "calls" in seam:
        seam["calls"] = calls
    # A switch waiting for the chief's turn, as the dispatcher says it of the chief's lane.
    if seam.get("method") == "pendingSwitch" and isinstance(seam.get("result"), dict) \
            and "agent" in seam["result"]:
        mover.count("pending switch")
        seam["result"] = mover.side(seam["result"])
    for key in ("result", "answer"):
        if key in seam:
            seam[key] = mover.value(seam[key])
    return seam


def main():
    family, files = sys.argv[1], sys.argv[2:]
    if family == "engine":
        models = kit_models()
        mover = Mover(lambda agent: models.get(agent, agent))
        move = move_engine
    elif family == "ledger":
        mover = Mover(lambda agent: agent)
        move = move_ledger
    elif family in ("page", "daemon"):
        roster = {}
        presets = {preset["id"]: preset for preset in json.load(open(PRESETS, encoding="utf-8"))["presets"]}
        known = lambda agent: roster.get(agent) or presets.get(agent)
        mover = Mover(lambda agent: (known(agent) or {}).get("model") or agent)
        mover.known = known
        move = lambda trace, mover: move_page(trace, mover, roster)
    else:
        raise SystemExit(f"no family {family}: engine, ledger or daemon")
    written = 0
    for path in files:
        raw = gzip.open(path, "rb").read()
        trace = json.loads(raw)
        before = json.dumps(trace, sort_keys=True)
        moved = move(trace, mover)
        if json.dumps(moved, sort_keys=True) != before:
            with open(path, "wb") as out:
                out.write(gzip.compress(json.dumps(moved, separators=(",", ":"), ensure_ascii=False).encode("utf-8"), 9, mtime=0))
            written += 1
    print(f"{family}: {written} of {len(files)} recordings moved")
    for what, count in sorted(mover.counts.items()):
        print(f"  {what}: {count}")


if __name__ == "__main__":
    main()
