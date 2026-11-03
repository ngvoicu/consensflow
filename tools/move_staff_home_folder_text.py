"""Moves cf-engine's role-text golden to the worker roles' sentence about the home folder (T-1194).

`skill/core/staff.md` says, after the paragraph about removals, that a window looks for files
inside the project folder and never over the home folder or in `~/Music` or `~/Pictures`, since on
a Mac macOS then asks the person at the screen in ConsensFlow's name and the command waits. Every
text of a staff role (worker, advisor, reviewer, tester, designer) holds it. A text already moved
is left as it is, so the move can be run again after a merge brings unmoved rows.

    python3 -I tools/move_staff_home_folder_text.py crates/cf-engine/tests/goldens/text.json
"""

import json
import sys

REMOVALS = 'through. Write a removal\'s paths out in full, or as `"${R:?}/${b:?}"`.\n'
SENTENCE = (
    "\nLook for files inside the project folder, never over the home folder or in\n"
    "`~/Music` or `~/Pictures` (`find ~`, `grep -r ~`, `ls ~/Music`): on a Mac that\n"
    "makes macOS ask the person at the screen, in ConsensFlow's name, and your command\n"
    "waits until they answer.\n"
)


def move(rows):
    """Moves `rows` in place; how many texts moved."""
    moved = 0
    for row in rows:
        answer = row.get("answer") or {}
        text = answer.get("text")
        if text and REMOVALS in text and SENTENCE not in text:
            answer["text"] = text.replace(REMOVALS, REMOVALS + SENTENCE, 1)
            moved += 1
    return moved


def main(paths):
    for path in paths:
        with open(path, encoding="utf-8") as file:
            tables = json.loads(file.read())
        moved = move(tables["roleInstructions"])
        with open(path, "w", encoding="utf-8") as file:
            file.write(json.dumps(tables, indent=2, ensure_ascii=False) + "\n")
        print(path, {"texts moved": moved})


if __name__ == "__main__":
    main(sys.argv[1:])
