"""Moves cf-harness's launch goldens to the prompt suggestion a Claude member's window has off (T-1434).

Node wrote a Claude window's settings file without an `env` block. The Rust adapter writes one into
every member's file, `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` as `false` (`claude/install.rs`,
`PROMPT_SUGGESTION_SWITCH`): after each turn Claude Code makes a call of its own to guess the
person's next prompt, and in a member's window nobody reads it. A member's file is the one whose
`PreToolUse` holds the board's question hook (`AskUserQuestion`); the chief's, whose `PreToolUse`
is empty, is not moved. A file already moved is left as it is, so the move can be run again after a
merge brings unmoved rows. Both platforms' recordings move: the switch is the same on each.

    python3 -I tools/move_claude_member_suggestion_goldens.py <scenarios.<platform>.json>...
"""

import json
import sys

MEMBER_HOOK = '"PreToolUse":[{"matcher":"AskUserQuestion"'
ENV = ',"env":{"CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION":"false"}'


def move(node):
    """Moves `node` in place; how many settings files moved."""
    moved = 0
    if isinstance(node, dict):
        for key, value in node.items():
            if (
                key == "text"
                and isinstance(value, str)
                and value.startswith('{"permissions"')
                and MEMBER_HOOK in value
                and ENV not in value
            ):
                assert value.endswith("}"), value
                node[key] = value[:-1] + ENV + "}"
                moved += 1
            else:
                moved += move(value)
    elif isinstance(node, list):
        for value in node:
            moved += move(value)
    return moved


def main(paths):
    for path in paths:
        with open(path, encoding="utf-8") as file:
            scenarios = json.loads(file.read())
        moved = move(scenarios)
        with open(path, "w", encoding="utf-8") as file:
            file.write(
                "[\n"
                + ",\n".join(
                    json.dumps(scenario, ensure_ascii=False, separators=(",", ":"))
                    for scenario in scenarios
                )
                + "\n]\n"
            )
        print(path, {"settings files moved": moved})


if __name__ == "__main__":
    main(sys.argv[1:])
