"""Moves cf-harness's Codex launch goldens to a role on no command line (T-698).

Node gave a Codex window its role as `-c developer_instructions=<JSON>`; the Rust
adapter writes the same text to `integrations/codex/<launch>/developer-instructions.md`
and names it in `CF_CODEX_ROLE_FILE`. For each Codex step that still has the setting
this drops its two words, adds the variable to the launch's environment after the
bridge, and adds the folders the scenario has not made yet and the file (its text the
setting's JSON string read, mode 0700 and 0600 on darwin, none on win32) to the
step's `tree`, where the file is new or its text changed; a step that closes the
window gets them `removed`. A step already moved has no setting and is left as it is,
so the move can be run again after a merge brings unmoved rows.

    python3 -I tools/move_codex_role_goldens.py <scenarios.json> <darwin|win32> [test-output]

`test-output` is what `cargo test -p cf-harness --test launch scenarios::` printed
before the move: each `rust` reading there is checked equal to the moved step.
"""

import json
import sys


def utf16(text):
    return text.encode("utf-16-be")


def move(scenarios, platform):
    """Moves `scenarios` in place; the moved steps, by scenario name and step."""
    mode = (lambda bits: bits) if platform == "darwin" else (lambda bits: None)
    moved = {}
    for scenario in scenarios:
        if scenario["harness"] != "codex":
            continue
        assert scenario["env"].get("CONSENSFLOW_HOME", "$ROOT/consensflow") == "$ROOT/consensflow"
        exists = {}
        last = None
        for record in scenario["records"]:
            tree = record.get("tree") or []
            if "close" in scenario["steps"][record["step"]]:
                said = {entry["path"] for entry in tree if entry.get("removed")}
                gone = [path for path in exists if last and (path == last or path.startswith(last + "/"))]
                adds = [{"path": path, "removed": True} for path in gone if path not in said]
                for path in gone:
                    del exists[path]
                if adds:
                    record["tree"] = sorted(tree + adds, key=lambda entry: utf16(entry["path"]))
                    moved[(scenario["name"], record["step"])] = record
                continue
            for entry in tree:
                if entry.get("removed"):
                    exists.pop(entry["path"], None)
                else:
                    exists[entry["path"]] = entry.get("text")
            adds = []
            for settled in record.get("settled", []):
                answer = settled.get("answer")
                if not (isinstance(answer, dict) and isinstance(answer.get("argv"), list)):
                    continue
                argv = answer["argv"]
                at = [i for i, word in enumerate(argv) if isinstance(word, str) and word.startswith("developer_instructions=")]
                launch = scenario["steps"][settled["op"]].get("prepare", {}).get("launchId")
                if launch:
                    last = f"$ROOT/consensflow/integrations/codex/{launch}"
                if not at:
                    continue
                (i,) = at
                assert argv[i - 1] == "-c"
                text = json.loads(argv[i][len("developer_instructions="):])
                del argv[i - 1:i + 1]
                moved[(scenario["name"], record["step"])] = record
                folders = ["$ROOT/consensflow", "$ROOT/consensflow/integrations",
                           "$ROOT/consensflow/integrations/codex", last]
                file = f"{last}/developer-instructions.md"
                answer["env"].append(["CF_CODEX_ROLE_FILE", file])
                for folder in folders:
                    if folder not in exists:
                        adds.append({"path": folder, "mode": mode(0o700)})
                        exists[folder] = None
                if exists.get(file, object()) != text:
                    adds.append({"path": file, "mode": mode(0o600), "text": text})
                    exists[file] = text
            if adds:
                record["tree"] = sorted(tree + adds, key=lambda entry: utf16(entry["path"]))
    return moved


def readings(output):
    """The `rust` readings a failed scenarios test printed, by scenario name and step."""
    found = {}
    lines = output.split("\n")
    for k, line in enumerate(lines):
        if line.startswith("  rust "):
            name, _, step = lines[k - 1].rpartition(", step ")
            found[(name, int(step))] = json.loads(line[len("  rust "):])
    return found


def main(args):
    path, platform = args[0], args[1]
    with open(path, encoding="utf-8") as file:
        scenarios = json.load(file)
    moved = move(scenarios, platform)
    print(f"{path}: {len(moved)} steps moved")
    if len(args) > 2:
        with open(args[2], encoding="utf-8") as file:
            rust = readings(file.read())
        wrong = [key for key, reading in rust.items() if moved.get(key) != reading]
        print(f"{len(rust)} readings of Rust's, {len(rust) - len(wrong)} equal to the move")
        if wrong or len(rust) != len(moved):
            for key in wrong[:5]:
                print("differs:", key)
            return 1
    with open(path, "w", encoding="utf-8") as file:
        file.write("[\n" + ",\n".join(json.dumps(scenario, ensure_ascii=False, separators=(",", ":")) for scenario in scenarios) + "\n]\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
