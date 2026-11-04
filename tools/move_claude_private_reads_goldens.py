"""Moves cf-harness's darwin launch goldens to the reads a Claude window on a Mac is denied (T-1194).

Node wrote a Claude window's settings file with `permissions` holding the permission mode alone.
On a Mac the Rust adapter adds `deny`, the four reads of the Apple Music library and the Photos
libraries that macOS asks about in the app's name (`claude/install.rs`, `private_reads`), unless
the window's folder is inside one of them. Every Claude scenario of the recording opens on
`/work/app`, so each file it recorded gets all four. A file already moved is left as it is, so
the move can be run again after a merge brings unmoved rows. Windows' recording is not moved:
the adapter denies nothing there.

    python3 -I tools/move_claude_private_reads_goldens.py <scenarios.darwin.json>...
"""

import json
import sys

BEFORE = '"permissions":{"defaultMode":"bypassPermissions"}'
AFTER = (
    '"permissions":{"defaultMode":"bypassPermissions","deny":'
    '["Read(~/Music)","Read(~/Music/**)",'
    '"Read(~/Pictures/*.photoslibrary)","Read(~/Pictures/*.photoslibrary/**)"]}'
)


def move(node):
    """Moves `node` in place; how many settings files moved."""
    moved = 0
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "text" and isinstance(value, str) and BEFORE in value and AFTER not in value:
                node[key] = value.replace(BEFORE, AFTER, 1)
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
