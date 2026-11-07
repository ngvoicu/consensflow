import re, subprocess, sys
repo, research, old, new = sys.argv[1:5]
text = open(research).read()
def git(*a):
    return subprocess.run(["git", "-C", repo] + list(a), capture_output=True, text=True).stdout
files_old = git("ls-tree", "-r", "--name-only", old).split("\n")
refs = []
for m in re.finditer(r"`?([A-Za-z0-9_./-]+\.(?:rs|js|html|sql|mjs|toml|md)):(\d+)(?:-(\d+))?", text):
    refs.append((m.group(1), int(m.group(2)), int(m.group(3) or m.group(2))))
# Lines given after a comma in the same mention: `dispatcher.rs:73-82, 192-226`
for m in re.finditer(r"([A-Za-z0-9_./-]+\.(?:rs|js|html|sql))(:\d+(?:-\d+)?(?:,\s*\d+(?:-\d+)?)+)", text):
    for piece in m.group(2)[1:].split(","):
        a, _, b = piece.strip().partition("-")
        refs.append((m.group(1), int(a), int(b or a)))
seen, rows = set(), []
for path, a, b in refs:
    if (path, a, b) in seen: continue
    seen.add((path, a, b))
    cands = [f for f in files_old if f == path or f.endswith("/" + path)]
    # A bare name the research gives within a crate's paragraph: the file it means.
    PREFER = {"windows.rs": "crates/cf-engine/src/windows.rs", "resume.rs": "crates/cf-engine/src/resume.rs",
              "chief_switch.rs": "crates/cf-engine/src/chief_switch.rs", "chief.rs": "crates/cf-ledger/src/conversations/chief.rs",
              "contract.rs": "crates/cf-harness/src/contract.rs", "handoff.rs": "crates/cf-engine/src/handoff.rs",
              "history.rs": "crates/cf-ledger/src/conversations/history.rs", "items.rs": "crates/cf-harness/src/claude/record/items.rs",
              "projects.rs": "crates/cf-ledger/src/projects.rs", "reading.rs": "crates/cf-harness/src/shared/record/reading.rs",
              "role_change.rs": "crates/cf-engine/src/role_change.rs", "staff.rs": "crates/cf-daemon/src/page/staff.rs",
              "text.rs": "crates/cf-engine/src/handoff/text.rs", "transcripts.rs": "crates/cf-engine/src/transcripts.rs",
              "views.rs": "crates/cf-ledger/src/views.rs", "index.html": "app/ui/index.html"}
    if len(cands) > 1 and PREFER.get(path) in cands:
        cands = [PREFER[path]]
    if len(cands) != 1:
        rows.append((path, a, b, "?", "path %s" % ("ambiguous: %d" % len(cands) if cands else "not found at the old base"))); continue
    full = cands[0]
    old_lines = git("show", "%s:%s" % (old, full)).split("\n")
    new_body = git("show", "%s:%s" % (new, full))
    if not new_body:
        rows.append((full, a, b, "-", "file gone on main")); continue
    new_lines = new_body.split("\n")
    first = old_lines[a - 1] if a - 1 < len(old_lines) else ""
    if new_lines[a - 1:a] == [first] and new_lines[b - 1:b] == old_lines[b - 1:b]:
        continue  # in place
    hits = [i + 1 for i, l in enumerate(new_lines) if l == first and first.strip()]
    if len(hits) == 1:
        rows.append((full, a, b, hits[0], "moved by %+d" % (hits[0] - a)))
    elif hits:
        near = min(hits, key=lambda h: abs(h - a))
        rows.append((full, a, b, near, "moved by %+d (the line occurs %d times; nearest)" % (near - a, len(hits))))
    else:
        rows.append((full, a, b, "?", "its first line changed: %r" % first.strip()[:70]))
print("%d references, %d in place, %d moved or changed" % (len(seen), len(seen) - len(rows), len(rows)))
for r in sorted(rows):
    print("%s:%s-%s -> %s  %s" % r)
