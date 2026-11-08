"""Moves cf-harness's launch goldens to the hyperlink switch each Claude Code and Pi launch carries (T-1315).

The pane is xterm.js and draws OSC 8 hyperlinks, but Claude Code (2.1.319) and Pi (1.5.0) know
only the terminals they list and printed a link as `words (address)` text, which the pane cannot
read to its end when the program wrote the address over rows of its own. A launch now tells each:
Claude Code `FORCE_HYPERLINK=1`, Pi `PI_HYPERLINKS=1`, as the last variable of its environment
(`claude/adapter.rs`, `pi/adapter.rs`). Node had neither. Every prepared launch of those two
harnesses in the recording gets its pair; a launch that holds it already is left as it is, so the
move can be run again after a merge brings unmoved rows. Codex, Devin and Muse have no such switch
and are not moved.

    python3 -I tools/move_harness_hyperlink_goldens.py <scenarios.<platform>.json>...
"""

import json
import sys

SWITCH = {"claude-code": ["FORCE_HYPERLINK", "1"], "pi": ["PI_HYPERLINKS", "1"]}


def move(node, pair):
    """Moves `node` in place; how many launches moved."""
    moved = 0
    if isinstance(node, dict):
        env = node.get("env")
        if "argv" in node and "dropEnv" in node and isinstance(env, list) and pair not in env:
            env.append(list(pair))
            moved += 1
        for value in node.values():
            moved += move(value, pair)
    elif isinstance(node, list):
        for value in node:
            moved += move(value, pair)
    return moved


def main(paths):
    for path in paths:
        with open(path, encoding="utf-8") as file:
            scenarios = json.loads(file.read())
        moved = {}
        for scenario in scenarios:
            pair = SWITCH.get(scenario["harness"])
            if pair is not None:
                moved[scenario["harness"]] = moved.get(scenario["harness"], 0) + move(scenario, pair)
        with open(path, "w", encoding="utf-8") as file:
            file.write(
                "[\n"
                + ",\n".join(
                    json.dumps(scenario, ensure_ascii=False, separators=(",", ":"))
                    for scenario in scenarios
                )
                + "\n]\n"
            )
        print(path, {"launches moved": moved})


if __name__ == "__main__":
    main(sys.argv[1:])
