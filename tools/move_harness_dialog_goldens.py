"""Moves cf-harness's launch goldens to the offers a launch turns off (T-1183).

Node started Claude Code with no variable of its own and Codex with two settings. The Rust
adapters start every Claude Code window with `CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1` (its
surveys wait under the prompt for a digit), and every Codex window with three settings more
after `allow_login_shell=false`: `notice.hide_rate_limit_model_nudge=true`,
`features.tool_suggest=false` and `features.skill_mcp_dependency_install=false`, each after its
`-c`. A step already moved is left as it is, so the move can be run again after a merge brings
unmoved rows.

    python3 -I tools/move_harness_dialog_goldens.py <scenarios.json>...
"""

import json
import sys

SURVEY = ["CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY", "1"]
OFFERS = [
    "-c",
    "notice.hide_rate_limit_model_nudge=true",
    "-c",
    "features.tool_suggest=false",
    "-c",
    "features.skill_mcp_dependency_install=false",
]


def move(scenarios):
    """Moves `scenarios` in place; how many answers moved, by harness."""
    moved = {"claude-code": 0, "codex": 0}
    for scenario in scenarios:
        harness = scenario["harness"]
        if harness not in moved:
            continue
        for record in scenario["records"]:
            for settled in record.get("settled", []):
                answer = settled.get("answer")
                if not (isinstance(answer, dict) and isinstance(answer.get("argv"), list)):
                    continue
                if harness == "claude-code":
                    env = answer.setdefault("env", [])
                    if SURVEY not in env:
                        env.insert(0, list(SURVEY))
                        moved[harness] += 1
                else:
                    argv = answer["argv"]
                    if "notice.hide_rate_limit_model_nudge=true" in argv:
                        continue
                    at = argv.index("allow_login_shell=false")
                    argv[at + 1:at + 1] = OFFERS
                    moved[harness] += 1
    return moved


def main(paths):
    for path in paths:
        with open(path, encoding="utf-8") as file:
            text = file.read()
        scenarios = json.loads(text)
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
        print(path, moved)


if __name__ == "__main__":
    main(sys.argv[1:])
