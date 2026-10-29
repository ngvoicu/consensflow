"""Drops the fileUrl table (to_file_url's only test data) from the launch goldens,
and its row from coverage.json. Nothing else may move (T-569, RMOC-20 of the harness
removal: `cf-base::file_url` had no caller left).

Run from the repository root, once: it asserts the table has its 295 rows, and the
asserts tell a tree it was already run on.
"""
import json
D = 'crates/cf-harness/tests/goldens/launch/'

def counts(t):
    return {k: (len(v) if isinstance(v, list) else len(json.dumps(v))) for k, v in t.items()}

s = open(D + 'tables.json').read()
t = json.loads(s)
assert json.dumps(t, indent=2, ensure_ascii=False) + '\n' == s, 'layout round-trips'
before = counts(t)
assert len(t['fileUrl']) == 295
del t['fileUrl']
out = json.dumps(t, indent=2, ensure_ascii=False) + '\n'
after = counts(json.loads(out))
assert {k: v for k, v in before.items() if k != 'fileUrl'} == after
open(D + 'tables.json', 'w').write(out)
print('tables.json', before, '->', after)

c = open(D + 'coverage.json').read()
row = '    "recordState": { "rows": 16, "held": "shared::record_state" },\n    "fileUrl": { "rows": 295, "held": "tests/launch/tables.rs" }\n'
assert c.count(row) == 1
before = json.loads(c)
c2 = c.replace(row, '    "recordState": { "rows": 16, "held": "shared::record_state" }\n')
after = json.loads(c2)
del before['tables']['fileUrl']
assert before == after
open(D + 'coverage.json', 'w').write(c2)
print('coverage.json tables', len(json.loads(c)['tables']), '->', len(after['tables']))
