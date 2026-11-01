"""Moves cf-harness's launch goldens to sub-agents that start without the window's role (T-766, T-1053).

Two files of a launch moved:

- Claude Code: the role's file (`integrations/claude/<launch>/role/.claude/skills/consensflow-<role>/SKILL.md`)
  opens with `disable-model-invocation: true` in its front matter, or in a front matter of its own
  before a text that has none, so that no model, a sub-agent's included, lists or loads it.
- Codex: beside the role's file (`integrations/codex/<launch>/developer-instructions.md`) is
  `sub-agents.toml`, the configuration a sub-agent starts with in place of the role (Codex's own
  instructions, then the sentence that it is not the window's agent), with the role file's mode;
  and the launch's environment names it in `CF_CODEX_SUBAGENT_FILE`, after the role's variable,
  and the agent types it is for in `CF_CODEX_SUBAGENT_TYPES` after that (Codex's own three: no
  recording's configuration defines one of its own).

A step's `tree` is what it changed of the files, so each scenario is replayed: its files after each
step, as its recorded changes leave them, are moved as above, and the step's `tree` is said again
as the change between the moved files before it and after it. A scenario already moved is moved to
itself, so the move can be run again after a merge brings unmoved rows.

    python3 -I tools/move_subagent_role_goldens.py crates/cf-harness/tests/goldens/launch/scenarios.<platform>.json
"""

import json
import re
import sys

NOT_FOR_THE_MODEL = "disable-model-invocation: true"
CLAUDE_ROLE = re.compile(r"/integrations/claude/[^/]+/role/\.claude/skills/consensflow-[^/]+/SKILL\.md$")
CODEX_ROLE = re.compile(r"/integrations/codex/[^/]+/developer-instructions\.md$")
ROLE_SAID = "\n\nYour ConsensFlow role is "
ROLE_VARIABLE = "CF_CODEX_ROLE_FILE"
SUBAGENT_VARIABLE = "CF_CODEX_SUBAGENT_FILE"
SUBAGENT_TYPES_VARIABLE = "CF_CODEX_SUBAGENT_TYPES"
AGENT_TYPES = "default,explorer,worker"


def not_for_the_model(text):
    """`claude::install::not_for_the_model`."""
    if text == "" or text.startswith(f"---\n{NOT_FOR_THE_MODEL}\n"):
        return text
    if text.startswith("---\n"):
        return f"---\n{NOT_FOR_THE_MODEL}\n{text[4:]}"
    return f"---\n{NOT_FOR_THE_MODEL}\n---\n\n{text}"


def toml_string(text):
    """`codex::role::toml_string`."""
    quoted = ['"']
    for character in text:
        if character == '"':
            quoted.append('\\"')
        elif character == "\\":
            quoted.append("\\\\")
        elif character == "\t":
            quoted.append("\t")
        elif character == "\n":
            quoted.append("\\n")
        elif character == "\r":
            quoted.append("\\r")
        elif ord(character) < 0x20 or ord(character) == 0x7F:
            quoted.append("\\u%04X" % ord(character))
        else:
            quoted.append(character)
    quoted.append('"')
    return "".join(quoted)


def children(role):
    """`codex::role::children`."""
    return (f"You are a sub-agent that a ConsensFlow {role} started. You are not the {role}: you have no "
            "board identity, so never run cf; answer your parent in your final message.")


def sub_agents(role_text):
    """The text of `sub-agents.toml` beside a role's file that holds `role_text`."""
    at = role_text.find(ROLE_SAID)
    assert at >= 0, role_text[:200]
    existing = role_text[:at]
    role = role_text[at + len(ROLE_SAID):].split(".", 1)[0]
    return f"developer_instructions = {toml_string(existing + chr(10) * 2 + children(role))}\n"


def sibling(path):
    return path[: path.rindex("/") + 1] + "sub-agents.toml"


def moved(files):
    """`files` (path to entry) as the moved code leaves them."""
    out = {}
    for path, entry in files.items():
        if CLAUDE_ROLE.search(path) and "text" in entry:
            entry = dict(entry, text=not_for_the_model(entry["text"]))
        out[path] = entry
    for path, entry in files.items():
        if CODEX_ROLE.search(path) and "text" in entry:
            out[sibling(path)] = {"mode": entry.get("mode"), "text": sub_agents(entry["text"])}
    return out


def utf16(text):
    return text.encode("utf-16-be")


def changes(before, after):
    """The scenarios suite's `changes`: each path made, changed or removed, in JavaScript's order."""
    said = []
    for path in sorted(set(before) | set(after), key=utf16):
        if path not in after:
            said.append({"path": path, "removed": True})
        elif before.get(path) != after[path]:
            said.append({"path": path, **after[path]})
    return said


def move_env(record):
    for settled in record.get("settled", []):
        answer = settled.get("answer")
        if not isinstance(answer, dict) or not isinstance(answer.get("env"), list):
            continue
        env = answer["env"]
        names = [pair[0] for pair in env]
        if ROLE_VARIABLE in names and SUBAGENT_VARIABLE not in names:
            at = names.index(ROLE_VARIABLE)
            env.insert(at + 1, [SUBAGENT_VARIABLE, sibling(env[at][1])])
            names = [pair[0] for pair in env]
        if SUBAGENT_VARIABLE in names and SUBAGENT_TYPES_VARIABLE not in names:
            # No recording's configuration defines an agent type of its own.
            env.insert(names.index(SUBAGENT_VARIABLE) + 1, [SUBAGENT_TYPES_VARIABLE, AGENT_TYPES])


def move(scenarios):
    changed = 0
    for scenario in scenarios:
        if scenario["harness"] not in ("claude-code", "codex"):
            continue
        files, was = {}, {}
        for record in scenario["records"]:
            before = json.dumps(record, sort_keys=True)
            for entry in record.get("tree") or []:
                if entry.get("removed"):
                    files.pop(entry["path"], None)
                else:
                    files[entry["path"]] = {key: value for key, value in entry.items() if key != "path"}
            now = moved(files)
            tree = changes(was, now)
            if "tree" in record or tree:
                record["tree"] = tree
            was = now
            if scenario["harness"] == "codex":
                move_env(record)
            changed += before != json.dumps(record, sort_keys=True)
    return changed


def main():
    path = sys.argv[1]
    with open(path, encoding="utf-8") as file:
        scenarios = json.load(file)
    changed = move(scenarios)
    # The recordings' own layout: one scenario a line, as compact as JSON goes.
    lines = (json.dumps(scenario, separators=(",", ":"), ensure_ascii=False) for scenario in scenarios)
    with open(path, "w", encoding="utf-8") as file:
        file.write("[\n" + ",\n".join(lines) + "\n]\n")
    print(f"{path}: {changed} steps moved")


if __name__ == "__main__":
    main()
