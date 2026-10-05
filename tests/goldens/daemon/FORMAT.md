# What Node answers on every surface of the daemon: the traces and their format

Step 3.6 ports the daemon around the engine to Rust, and three workers port its
surfaces: the agents' API, the page operations and the screens. Each holds its
Rust to what Node answers, recorded from the suites that exist (the oracle of
`decision-36`). This is the format of every file in
`crates/cf-daemon/tests/goldens/`, for the players that read them.

    node app/scripts/build-cf.mjs               first: the cf runs need the native cf in bin/
    npm run goldens:daemon                      runs the suites with the recorder in, writes the files
    node tests/goldens/daemon/record.mjs --check    records again into a folder of its own, says where it differs
    node tests/goldens/daemon/show.mjs core-api-001   one trace, a line a step

The recorder is `tests/goldens/daemon/` (`hooks.mjs` puts a wrapper in the place
of each module a suite imports, and each wrapper imports the real one); the
ledger it stands on is step 3.1's (`tests/goldens/ledger/`). It changes no
outcome: every suite it records passes with it and without it.

The worked examples below (each JSON block after an `<!-- example: … -->`
marker is a step of a trace, each after a `<!-- file: … -->` marker a file of
the folder) are not typed: `npm run goldens:daemon` rewrites them from the
recording, and a test holds this document to the files that are checked in.

## What is in the folder

| Path | What | Read by |
|---|---|---|
| `<suite>-<NNN>.json.gz` | one trace per test that reached a surface, numbered from 001 in the order the suite's tests ran | the players |
| `pages/agents.html`, `pages/harnesses.html` | the two pages the screens serve, `$TOKEN` and `$VERSION` where the token and the version go | screens |
| `operations.json` | the 28 page operations in the order the page offers them, and the reply to `ping` | page |
| `daemon.json` | the handle line the daemon prints, and the lines it logs when it starts and stops | the skeleton |
| `files.json` | the formats of the daemon's log and trace lines, each case once as Node writes it with its clock fixed at 2026-10-05T10:00:00.123Z, their rotation past a limit, and what `forget` leaves of a trace | `crates/cf-daemon/tests/files.rs` |

| Suite | Traces | `surface` | Held by |
|---|---|---|---|
| `tests/core-api.test.mjs` | `core-api-001` to `007` | `api` | the API |
| `tests/core-daemon.test.mjs`, the door still waiting when the API closes | `core-daemon-001` | `api` | the API |
| `tests/integration/cf-board.test.mjs` | `cf-board-001` to `029` | `cf` | the API, and `cf` against it |
| `scenarios/corners-api.test.mjs` | `corners-api-001` to `010` | `api` | the API |
| `tests/core-page.test.mjs` | `core-page-001` to `028` | `page` | the page operations |
| `scenarios/corners-page.test.mjs` | `corners-page-001` to `008` | `page` | the page operations |
| `tests/core-agents-server.test.mjs` | `core-agents-server-001` to `009` | `screens` | the screens |
| `scenarios/corners-screens.test.mjs` | `corners-screens-001` to `010` | `screens` | the screens |
| `tests/core-trace.test.mjs` | `core-trace-001` to `002` | `trace` | the line formats |
| `tests/core-log.test.mjs` | `core-log-001` | `log` | the line formats |

(`scenarios/` is `tests/goldens/daemon/scenarios/`: what no suite looked at, the order of the API's checks, how it reads a target, a body and a number, what the screens write. They are Node's tests of Node, kept there because the ledger recorder takes every suite in `tests/`. Their words are in the traces.)

A screens trace holds the exchanges the API answers when the UI token opens none of its routes (`/api/whoami` with it is the API's 401): they are in the screens' traces, not the API's, because the screens mount the API beneath them.

## Reading the files

Every trace is JSON, gzipped, one line and a final line break. Read it as text first: the placeholders below are replaced in the text, as the ledger's replay replaces `«ledger»`, and then it is parsed.

**Names for what differs from one run to the next.** Nothing else in a trace varies from one run to the next.

| Placeholder | Stands for | A player |
|---|---|---|
| `«ledger»` | the path of the ledger's file | puts the path of its own |
| `«root»` | the folder the test made: `HOME`, `CONSENSFLOW_HOME`, `PATH`, the roster, the stand-ins on `PATH` | puts the folder of its own |
| `«api»` | `127.0.0.1:<port>`, the host and port the API listened on | puts the host and port of the API under test |
| `«token:T1»` | the token the step `issue` named `T1` gave | puts the token its credentials issued for that window; `T1`, `T2`… are the order they were issued in |
| `«now»` | a time the writer stamped from its own clock: a roster agent's `createdAt` and `updatedAt`, the `at` of a trace line whose entry had none, the time of a log line the test gave no clock | compares its own output with that time replaced by `«now»` |
| `«frame»` | one stack frame in the text of a logged error | none: the error is given as the text it is |

The ports and tokens are not in the traces: only these names are. A trace holds no path of the machine that made it, no token and no port. The recorder checks each trace as it makes it (`check.mjs`: step kinds, tokens issued before they are used, intervals settled, no temporary folder and no 64 random hex digits left), and `--check` records again and compares. That check of leftovers looks for the temporary folder of the machine it runs on: it is a guard at the recording, not on another machine; what keeps a path out of a trace is the naming.

**Values JSON cannot hold** are tagged as in step 3.1's traces: `{"$undefined":true}` (a key that was there, with no value: it differs from a key that was not), `{"$number":"NaN"}`, `{"$bigint":"…"}`, `{"$date":"…"}`, `{"$set":[…]}`, `{"$map":[[key,value]…]}`, `{"$fn":n}`, and `{"$error":{"name","code","status","message"}}` for what a call threw.

**Bodies** (requests, answers, a run's input and output) are the exact text, compared as bytes: key order, spacing and absent against null count. A body that is not UTF-8 is `bodyBase64` (a run's stream, `{"base64":…}`); no body at all is `null` (an empty run stream is `""`).

**The ledger's clock and names.** Each ledger the suites opened reads a clock that starts at `2026-01-01T00:00:00.000Z` and reads one second later each time, and draws its session names from one fixed sequence, unless the test gave its own (`ledger.options` says which): that is what makes two recordings the same bytes. The readings and names a call took are written where the call is (`clock`, `names`); a player puts them in the ledger's queues before the call and finds them taken after it, as 3.1's replay does.

## A trace

| Key | |
|---|---|
| `format` | `1` |
| `surface` | `api`, `cf`, `page`, `screens`, `trace` or `log`: the first of these the test reached, narrowest first |
| `test` | `{file, path}`: the suite's file, and the `describe`s and the test's name; no line, which would change every trace after an edit to a suite |
| `ledger` | `null`, or the ledger the test opened (below) |
| `ui` | `{token}` when the screens were mounted: the UI token the test gave them |
| `steps` | what happened, in order |

`ledger` is `{file, options, initial?, openError?, final, unclosed?}`. `file` is `«ledger»`. `options` says whether the test gave the ledger its own `now`, `names` and `trace` (`false`: the recorder's). `initial`, as in step 3.1's traces, is what a file there before the open held. `final` is the database after the ledger's `close` step, to compare exactly: `{userVersion, schema, tables: {name: {columns, rows}}}`, each value as SQLite quotes it. A ledger the test never closed has `final: null` and `unclosed: true`. A test that opened no ledger has `null`: the traces of the lines, and those of `core-agents-server`, whose suite opens its ledger before its tests and closes it after.

## Steps

Each step is an object with a `kind`.

| `kind` | A player |
|---|---|
| `ledger` | makes the call on its ledger |
| `issue`, `revoke` | issues a token for a window, gives one back |
| `exchange` | sends the request to the API, compares the answer |
| `api.close` | closes the API, waits for it |
| `settle` | waits here for the exchange, run or close that was left running |
| `run` | runs the native `cf`, compares what it printed |
| `operation` | asks the page operation, compares its reply |
| `world` | puts files and variables in place |
| `kick` | (a dispatcher kick that no exchange or operation owned: none is recorded) |
| `trace.open`, `trace.append`, `trace.forget`, `log.open`, `log.write` | the line formats |

### `ledger`

A call the test made on the ledger **that changed something**, as step 3.1's traces hold a call: `method`, `args`, `clock`, `names`, `events`, `callbacks`, `result`. The code that replays those (`crates/cf-ledger/tests/replay.rs`) replays these. The last of a trace is the ledger's `close`: its `final` is `ledger.final`.

<!-- example: cf-board-001 steps.3 -->
```json
{
  "kind": "ledger",
  "method": "beginDelivery",
  "args": [
    1
  ],
  "clock": [
    "2026-01-01T00:00:09.000Z"
  ],
  "names": [],
  "events": [
    {
      "at": "2026-01-01T00:00:09.000Z",
      "project": 1,
      "kind": "delivery.begun",
      "data": {
        "message": 1,
        "attempt": 1
      }
    }
  ],
  "callbacks": [],
  "result": {
    "id": 1,
    "projectId": 1,
    "recipient": "zeus",
    "recipientId": 3,
    "recipientRole": "worker",
    "sender": "chief",
    "kind": "task",
    "taskNumber": 1,
    "replyTo": null,
    "body": "Parser",
    "state": "delivering",
    "attempts": 1,
    "reason": null,
    "receipt": null,
    "questions": null,
    "choices": null,
    "urgent": false,
    "createdAt": "2026-01-01T00:00:07.000Z",
    "deliveredAt": null
  }
}
```

A call the test made that only reads (`project`, `task`, `inbox`, `message`, `pending`, `projects`, `board`…, each that drew no reading and logged nothing) is left out: it changes nothing a player must repeat, and a test that waits for a window's question makes as many as the machine is slow. The calls the API and the page operations make on the ledger while they answer are not steps either: see `exchange` and `operation`.

### `issue`, `revoke`

`issue` is `credentials.issue({participant, project})`: the window is the participant (`id`, `handle`) of the project (`id`), the ids the ledger's own steps gave, and the token is named `T1`, `T2`… in the order they were issued. A player issues one for the same ids on its own credentials and maps the name to what it gave. `revoke` gives one back: `token` is the name, or the text the test passed when no `issue` gave it.

<!-- example: core-api-001 steps.2 -->
```json
{
  "kind": "issue",
  "token": "T1",
  "project": 1,
  "participant": {
    "id": 2,
    "handle": "chief"
  }
}
```

### `exchange`

One HTTP exchange with the API, as the server saw it.

| Key | |
|---|---|
| `id` | 1, 2… in the order the requests arrived |
| `client` | `test`: the test's own code; `cf`: the native `cf` (its user agent is `ureq/…`) |
| `run` | for a `cf` exchange, the `run` it belongs to |
| `request` | `{method, target, authorization, contentType, body}` |
| `response` | `{status, contentType, body}`, or `null` when `aborted` |
| `kicks` | how many times the API called `changed()` while it answered: the dispatcher's wake-ups |
| `clock`, `names`, `events` | what the API's own ledger calls read, drew and logged, in the order they came |
| `seams` | the calls the API made on the test's stand-ins (the roster lookup), below |
| `wrote` | the files this exchange changed, each as it was and as it is, below |
| `screens` | `true` when the agents screens answered, not the API |
| `detached` | `true` when other steps began before it ended: a `settle` marks where it ended |
| `aborted` | `true` when the connection closed before it was answered |

`request.target` is the request line's target, **as sent**: nothing is normalized (`/api/tasks/../whoami`, `//host/api/whoami` and `/api\whoami` are recorded as they came; Node reads a target as a WHATWG URL, and so reaches the same route). `authorization` is the header as sent, its tokens named, or `null` for none. `contentType` is the header or `null`. `body` is the bytes the client sent **whether or not the API read them**: a route that refuses before it reads a body leaves it to Node, and the trace has it (`truncated`, a number, is the length the client declared when its body stopped short: none of the recorded ones does).

`response.contentType` is the type the answer carried (`application/json`, `text/html; charset=utf-8`, `null` for none, as on a 204); `body` is its bytes, `null` for none (an answer to `HEAD` has none on the wire, whatever the handler wrote). A page the screens served (`GET /` and `GET /harnesses`) has `page` (`agents` or `harnesses`) in place of `body`: its body is that file under `pages/`, with `$TOKEN` and `$VERSION` filled in.

<!-- example: core-api-001 steps.3 -->
```json
{
  "kind": "exchange",
  "id": 1,
  "client": "test",
  "request": {
    "method": "POST",
    "target": "/api/tasks",
    "authorization": "Bearer «token:T1»",
    "contentType": "application/json",
    "body": "{\"tier\":\"standard\",\"body\":\"Write the parser\"}"
  },
  "response": {
    "status": 201,
    "contentType": "application/json",
    "body": "{\"task\":{\"number\":1,\"title\":\"Write the parser\",\"state\":\"open\",\"requester\":\"chief\",\"assignee\":null,\"pool\":\"worker\",\"tier\":\"standard\",\"needs\":[],\"blockedBy\":[],\"updatedAt\":\"2026-01-01T00:00:06.000Z\"},\"message\":null,\"gated\":false}"
  },
  "kicks": 1,
  "clock": [
    "2026-01-01T00:00:06.000Z",
    "2026-01-01T00:00:07.000Z"
  ],
  "names": [],
  "events": [
    {
      "at": "2026-01-01T00:00:07.000Z",
      "project": 1,
      "kind": "task.opened",
      "data": {
        "task": 1,
        "from": "chief",
        "pool": "worker",
        "tier": "standard"
      }
    }
  ],
  "seams": []
}
```

A request that is refused before its body is read still has it:

<!-- example: core-api-001 steps.5 -->
```json
{
  "kind": "exchange",
  "id": 2,
  "client": "test",
  "request": {
    "method": "POST",
    "target": "/api/tasks",
    "authorization": "Bearer «token:T2»",
    "contentType": "application/json",
    "body": "{\"tier\":\"standard\",\"body\":\"Do it\"}"
  },
  "response": {
    "status": 403,
    "contentType": "application/json",
    "body": "{\"error\":\"not-a-coordinator\",\"message\":\"members do not hand out tasks: ask your chief instead (cf ask)\"}"
  },
  "kicks": 0,
  "clock": [],
  "names": [],
  "events": [],
  "seams": []
}
```

**What the API's own ledger calls drew.** The calls the API makes on the ledger while it answers are not steps: a Rust API makes its own. What the trace keeps of them is what a player's ledger must be given and what it must show: `clock` and `names`, in order, to put in the queues before the request is sent (and find taken after the answer), and `events`, the events its calls logged, to compare with what the Rust ledger logged meanwhile. What they changed is in `ledger.final`. In the first example, `POST /api/tasks` read the clock twice and logged one event; the answer was built after the kick, as Node builds it.

**`seams`.** A stand-in a test gave the API is called while it answers: today the roster lookup of `/api/staff` (`seam: "roster"`, `method: "call"`). Each call is `{seam, method, args, calls, result | refusal}`, in the order it was made: the player's stand-in asserts the arguments and answers with `result`, or throws `refusal` (`{name, code, status, message}`). `calls` are the ledger steps a stand-in made while it served (none, for the roster).

<!-- example: cf-board-018 steps.4 -->
```json
{
  "kind": "exchange",
  "id": 1,
  "client": "cf",
  "run": 1,
  "request": {
    "method": "GET",
    "target": "/api/staff",
    "authorization": "Bearer «token:T1»",
    "contentType": "application/json",
    "body": null
  },
  "response": {
    "status": 200,
    "contentType": "application/json",
    "body": "{\"members\":[{\"handle\":\"zeus\",\"role\":\"worker\",\"roles\":[\"worker\"],\"tier\":\"standard\",\"harness\":\"claude-code\",\"model\":\"claude-sonnet-5\",\"effort\":\"high\"}]}"
  },
  "kicks": 0,
  "clock": [],
  "names": [],
  "events": [],
  "seams": [
    {
      "seam": "roster",
      "method": "call",
      "args": [
        "zeus"
      ],
      "calls": [],
      "result": {
        "id": "zeus",
        "kind": "claude-code",
        "model": "claude-sonnet-5",
        "effort": "high"
      }
    }
  ]
}
```

**`wrote`** (screens and page traces): `{ "<path under «root»>": {before, after} }`, each side `null` (not there) or `{text, executable?}` (`base64` for a file that is no UTF-8): the files this exchange changed. A roster file's `createdAt` and `updatedAt` are `«now»`: the time is the clock's. The screens' write routes leave the file as `after` says, and nothing else under `«root»`.

<!-- example: core-agents-server-007 steps.6 -->
```json
{
  "kind": "exchange",
  "id": 6,
  "client": "test",
  "request": {
    "method": "DELETE",
    "target": "/api/agents/mine",
    "authorization": "Bearer ui-token-for-the-tests",
    "contentType": "application/json",
    "body": null
  },
  "response": {
    "status": 204,
    "contentType": null,
    "body": null
  },
  "kicks": 0,
  "clock": [],
  "names": [],
  "events": [],
  "seams": [],
  "screens": true,
  "wrote": {
    "consensflow/agents.json": {
      "before": {
        "text": "{\n  \"schemaVersion\": 1,\n  \"agents\": [\n    {\n      \"id\": \"mine\",\n      \"name\": \"Mine\",\n      \"kind\": \"claude-code\",\n      \"createdAt\": \"«now»\",\n      \"updatedAt\": \"«now»\",\n      \"model\": \"claude-fable-5-1\",\n      \"workTier\": \"complex\"\n    }\n  ],\n  \"preferences\": {\n    \"ownHarnessOnly\": false\n  }\n}\n"
      },
      "after": {
        "text": "{\n  \"schemaVersion\": 1,\n  \"agents\": [],\n  \"preferences\": {\n    \"ownHarnessOnly\": false\n  }\n}\n"
      }
    }
  }
}
```

**`detached`.** Almost every exchange begins and ends with nothing between: a player sends it and compares the answer. One that other steps overlapped (a door held open while the API closes; a hook's long poll that the test answers meanwhile) is `detached`, and the `settle` step that names it (`{"kind":"settle","exchange":1}`) is where it ended: a player sends it and goes on, and compares the answer when it reaches the `settle`.

<!-- example: core-daemon-001 steps.4 -->
```json
{
  "kind": "exchange",
  "id": 1,
  "client": "test",
  "request": {
    "method": "GET",
    "target": "/api/questions/1?wait=25000",
    "authorization": "Bearer «token:T1»",
    "contentType": null,
    "body": null
  },
  "response": {
    "status": 200,
    "contentType": "application/json",
    "body": "{\"question\":{\"id\":1,\"kind\":\"question\",\"state\":\"queued\",\"sender\":\"zeus\",\"recipient\":\"chief\",\"task\":null,\"preview\":\"Which?\",\"questions\":null,\"choices\":null,\"createdAt\":\"2026-01-01T00:00:06.000Z\"},\"answer\":null}"
  },
  "kicks": 0,
  "clock": [],
  "names": [],
  "events": [],
  "seams": [],
  "detached": true
}
```

<!-- example: core-daemon-001 steps.5 -->
```json
{
  "kind": "api.close",
  "id": 1,
  "detached": true
}
```

<!-- example: core-daemon-001 steps.6 -->
```json
{
  "kind": "settle",
  "exchange": 1
}
```

<!-- example: core-daemon-001 steps.7 -->
```json
{
  "kind": "settle",
  "close": 1
}
```

### `api.close`

`api.close()` called and awaited: `{kind: "api.close", id}`; `detached` and its `settle` (`{kind: "settle", close: id}`) as above. A test that closes the API has one, before the ledger's `close`.

### `run`

One run of the native `cf` whole, in its place among the steps.

| Key | |
|---|---|
| `id` | 1, 2… |
| `argv` | the arguments, without the program |
| `env` | the variables it was given **beyond the rig's**: the rig starts from the test's own environment less every `CONSENSFLOW_*`, `CF_*` and `CHISEL_*` variable (a window has none of its own), and `env` is what the test added or changed, `CONSENSFLOW_URL` and `CONSENSFLOW_TOKEN` among them, named as above |
| `stdin` | what was written to it (`""` for nothing) |
| `stdout`, `stderr` | what it wrote, exact (the fixture strips one final line break; the trace does not) |
| `code`, `signal` | how it ended |
| `detached` | as for an exchange |

The exchanges with `client: "cf"` and this `run` are the requests the run made. A player that runs `cf` does not send them: the run does; they are what it should find the API doing, and where it waits (below). A player that does not run `cf` sends them like any other.

<!-- example: cf-board-001 steps.6 -->
```json
{
  "kind": "run",
  "id": 1,
  "argv": [
    "task",
    "get",
    "T-1",
    "--transcript"
  ],
  "env": {
    "CONSENSFLOW_URL": "http://«api»",
    "CONSENSFLOW_TOKEN": "«token:T1»"
  },
  "stdin": "",
  "stdout": "T-1 [working] @zeus ← @chief: Parser\n\nIts window has written nothing yet.\n",
  "stderr": "",
  "code": 0
}
```

<!-- example: cf-board-001 steps.8 -->
```json
{
  "kind": "exchange",
  "id": 2,
  "client": "cf",
  "run": 1,
  "request": {
    "method": "GET",
    "target": "/api/tasks/1/transcript?last=10",
    "authorization": "Bearer «token:T1»",
    "contentType": "application/json",
    "body": null
  },
  "response": {
    "status": 200,
    "contentType": "application/json",
    "body": "{\"total\":0,\"items\":[]}"
  },
  "kicks": 0,
  "clock": [],
  "names": [],
  "events": [],
  "seams": []
}
```

### `operation`

One page operation, called as the daemon calls it for the bridge's request.

| Key | |
|---|---|
| `id` | 1, 2… |
| `name` | one of `operations.json`'s |
| `body` | what the page sent |
| `kicks` | how many times the operation woke the dispatcher: the operations that change something kick once after their work, none when they are refused, and the reads never |
| `clock`, `names`, `events` | what the operation's own ledger calls read, drew and logged |
| `seams` | the calls it made on the test's stand-ins: the dispatcher, and a ledger the recorder did not open |
| `reply` | the text the daemon puts on the bridge: `{"ok":true,…}` with the operation's answer, or `{"ok":false,"error":"<words>"}` when it threw |
| `refusal` | when it threw: `{name, code, status, message}` |
| `wrote` | as for an exchange |

`reply` is compared as bytes. `message` is what a refusal says: the page shows it. A thrown value that is no `Error` (a string) has its text as `message` and no `code`.

A call on the dispatcher is `{seam: "dispatcher", method, args, calls, result | refusal}`: the stand-in asserts `method` and `args` (`{"$undefined":true}` is a key that was there with no value: `None`), replays `calls` (ledger steps, each with its readings, as top-level `ledger` steps are), and answers `result`, or throws `refusal`. A ledger the test passed that the recorder did not open (the one stand-in of `core-page`'s `staff.last` test) is `seam: "ledger"`, answered the same way.

<!-- example: core-page-020 steps.2 -->
```json
{
  "kind": "operation",
  "id": 2,
  "name": "project.delete",
  "body": {
    "project": 1
  },
  "kicks": 0,
  "clock": [],
  "names": [],
  "events": [],
  "seams": [
    {
      "seam": "dispatcher",
      "method": "deleteProject",
      "args": [
        1
      ],
      "calls": [
        {
          "kind": "ledger",
          "method": "deleteProject",
          "args": [
            1
          ],
          "clock": [],
          "names": [],
          "events": [],
          "callbacks": [],
          "result": {
            "$error": {
              "name": "LedgerError",
              "code": "project-open",
              "status": 409,
              "message": "app is open: close it first"
            }
          }
        }
      ],
      "refusal": {
        "name": "LedgerError",
        "code": "project-open",
        "status": 409,
        "message": "app is open: close it first"
      }
    }
  ],
  "reply": "{\"ok\":false,\"error\":\"app is open: close it first\"}",
  "refusal": {
    "name": "LedgerError",
    "code": "project-open",
    "status": 409,
    "message": "app is open: close it first"
  }
}
```

<!-- example: core-page-020 steps.4 -->
```json
{
  "kind": "operation",
  "id": 4,
  "name": "project.delete",
  "body": {
    "project": 1
  },
  "kicks": 1,
  "clock": [],
  "names": [],
  "events": [],
  "seams": [
    {
      "seam": "dispatcher",
      "method": "deleteProject",
      "args": [
        1
      ],
      "calls": [
        {
          "kind": "ledger",
          "method": "deleteProject",
          "args": [
            1
          ],
          "clock": [],
          "names": [],
          "events": [],
          "callbacks": [],
          "result": {
            "id": 1,
            "name": "app",
            "directory": "/work/app",
            "createdAt": "2026-01-01T00:00:00.000Z",
            "members": 0,
            "sessions": 0,
            "tasks": 0,
            "messages": 0
          }
        }
      ],
      "result": {
        "id": 1,
        "name": "app",
        "directory": "/work/app",
        "createdAt": "2026-01-01T00:00:00.000Z",
        "members": 0,
        "sessions": 0,
        "tasks": 0,
        "messages": 0
      }
    }
  ],
  "reply": "{\"ok\":true,\"project\":{\"id\":1,\"name\":\"app\",\"directory\":\"/work/app\",\"createdAt\":\"2026-01-01T00:00:00.000Z\",\"members\":0,\"sessions\":0,\"tasks\":0,\"messages\":0}}"
}
```

**The operation's clock and the stand-ins' do not mix.** An operation that reads the clock itself (`clock`) does not do so through a stand-in's ledger call too, in any recorded operation (the recorder refuses a trace that does). One queue of readings serves a player: put an operation's own before it runs, and a stand-in's ledger step's when the stand-in replays it.

### `world`

The files and variables the surfaces read, put in place before the step that reads them. The first `world` of a trace is whole: `{env, files}`, every variable the test's environment held (the paths under `«root»`) and every file under `«root»` (but the ledger's), so each trace starts from nothing. Later ones hold what changed since: a variable or file that went is `null`. A file is `{text, executable?}` (`executable` for a stand-in on `PATH`), or `{base64, executable?}` for bytes that are no UTF-8, at its path under `«root»`, `/`-separated. A roster file's stamps are `«now»`: a player writes its own.

<!-- example: core-agents-server-001 steps.0 -->
```json
{
  "kind": "world",
  "env": {
    "HOME": "«root»/home",
    "CONSENSFLOW_HOME": "«root»/consensflow",
    "CLAUDE_CONFIG_DIR": "«root»/home/.claude",
    "CODEX_HOME": "«root»/home/.codex",
    "XDG_CONFIG_HOME": "«root»/home/.config",
    "PATH": "«root»/bin",
    "CONSENSFLOW_BIN_DIR": "«root»/consensflow/bin"
  },
  "files": {
    "bin/claude": {
      "text": "#!/bin/sh\nexit 0\n",
      "executable": true
    }
  }
}
```

A world is written before an exchange or an operation, so the files are what the step reads, and the `wrote` of the step says what it changed.

### The lines

A trace of the event file or the daemon's log (`surface` `trace` or `log`) has no ledger: its steps are the calls the test made, in order, each with the files of the folder after it.

`trace.open` is `eventTrace(folder, {limit})`: `trace` is its number, `folder` is relative to `«root»` (`.` for the root itself), `limit` the byte limit past which the file is set aside. `trace.append` writes `entry` (a line `{"at":<now>,…entry}`: `at` is first, and the entry's own `at` wins); `trace.forget` is `forget(project)`. `files` is every file of the folder as it is after the step: `events.jsonl`, `events.jsonl.1` (the one older file, kept once), and the log's `daemon.log` beside them, each as text. A line the trace dated itself has `"at":"«now»"`. `log.open` is `daemonLog(folder, {limit, now})`: `clock: true` when the test gave a `now`, and each `log.write` then lists its reading in `clock`. `log.write` is `info`, `warn` or `error` with `message` and, when there is one, `error: {text}`: the text of a stack, which the log writes under the line indented by four, and which the recorder gave two frames named `«frame»`: a player gives the log that text as its error. A line the log dated itself (no `now`) starts with `«now»`. A folder that is not there (a home that cannot be written) leaves `files: {}`: the line is lost, the run goes on.

<!-- example: core-trace-001 steps.0 -->
```json
{
  "kind": "trace.open",
  "trace": 1,
  "folder": ".",
  "limit": 5000000
}
```

<!-- example: core-trace-001 steps.1 -->
```json
{
  "kind": "trace.append",
  "trace": 1,
  "entry": {
    "kind": "window.activity",
    "participant": "chief",
    "state": "idle"
  },
  "files": {
    "events.jsonl": "{\"at\":\"«now»\",\"kind\":\"window.activity\",\"participant\":\"chief\",\"state\":\"idle\"}\n"
  }
}
```

<!-- example: core-trace-001 steps.7 -->
```json
{
  "kind": "trace.forget",
  "trace": 1,
  "project": 1,
  "files": {
    "events.jsonl": "{\"at\":\"«now»\",\"kind\":\"window.activity\",\"participant\":\"chief\",\"state\":\"idle\"}\n{\"at\":\"«now»\",\"kind\":\"task.opened\",\"project\":2,\"data\":{\"task\":1}}\n{\"at\":\"«now»\",\"kind\":\"project.deleted\",\"project\":null,\"data\":{\"id\":1,\"name\":\"app\"}}\n"
  }
}
```

<!-- example: core-log-001 steps.0 -->
```json
{
  "kind": "log.open",
  "log": 1,
  "folder": ".",
  "limit": 200,
  "clock": true
}
```

<!-- example: core-log-001 steps.2 -->
```json
{
  "kind": "log.write",
  "log": 1,
  "level": "error",
  "message": "pass failed",
  "error": {
    "text": "Error: boom\n    at «frame»\n    at «frame»"
  },
  "clock": [
    "2026-09-24T14:00:01.000Z"
  ],
  "files": {
    "daemon.log": "2026-09-24T14:00:00.000Z info start pid 1\n2026-09-24T14:00:01.000Z error pass failed\n    Error: boom\n        at «frame»\n        at «frame»\n"
  }
}
```

## Playing a trace

All players: replace the placeholders in the text, parse, make the folder the test made (`«root»`) and the ledger's file, and walk `steps` in order. Compare at each step what it says; at the end compare the database at the ledger's `close` with `ledger.final`.

### The agents' API (`api`, and the exchanges of `cf`)

1. `ledger`: replay as 3.1's replay does (`clock` and `names` into the queues, the call, the answer or refusal, the events, the readings taken).
2. `issue`: issue a token for the window on the API's credentials; keep `T1` → token. `revoke`: give it back.
3. `exchange`: put its `clock` and `names` in the queues; send `request` (`method`, `target` as it is, `authorization` with the tokens put back, `contentType` when it is not null, the bytes of the body); compare the status, the type (when `contentType` is not null) and the body bytes (or the page, filled in); count the kicks the API makes (`kicks`); compare the events its ledger logged with `events`; find the queues empty. Its `seams` are answered in order by the player's roster. A `detached` one is sent and left until its `settle`.
4. `api.close`: close the API and wait; it must return, whatever is left waiting (a door's long poll is answered at once with `answer: null`).
5. The last step is the ledger's `close`.

The API's own calls are the Rust API's own: a player compares what they leave (the database, the events, the readings and names taken), not which calls they were.

### `cf` against the API (`cf`)

`cf-board-*` hold the same steps with `run`s. A player that runs the real `cf`: for a `run`, spawn the native `cf` with `argv`; an environment that is its own less every `CONSENSFLOW_*`, `CF_*` and `CHISEL_*`, plus `env` (named values put back); write `stdin`, close it; compare `stdout`, `stderr` and `code` exactly. Do not send the exchanges with `client: "cf"`: the run makes them, and what the API receives from it should be what they say. Where steps come between a `run` and its end (`detached`), the run is left running and compared at its `settle`; before a step of the test's own that follows an exchange of the run (a `POST /api/answers` after the run's question was put), wait until the API under test has received the run's exchanges recorded before it. The hook cases (`cf hook …`) are the two that do: `cf-board-023` and `024`.

### The page operations (`page`)

Per `operation`: put the `world` in place; set up the stand-ins from `seams` in order: the first call the operation makes on the dispatcher must be `seams[0]` (method and `args`), and so on, and none may be left over; put the operation's `clock` and `names` in the queues; call the operation with `body`; compare its reply to `reply` (as bytes), its kicks to `kicks`, a refusal's `message` to `refusal.message`, the events its ledger logged to `events`. `operations.json` lists the names and `ping`'s reply.

### The screens (`screens`)

As the API, with the UI token (`ui.token`) in place of a window's: put the first `world` in place (the stand-ins on `PATH`, the roster the earlier tests of the suite left, so each trace starts whole), mount the screens with that token, and for each exchange compare the answer and the `wrote` (the files it changed, as `after` says, and no other under `«root»`). The pages are `pages/` filled in. Exchanges without `screens` are the API's answer, beneath the screens: for a route they do not own, a 401 that is the API's (`error`, `message`), not their bare `{"error":"unauthorized"}`.

### The line formats (`trace`, `log`)

Make the folder, call the Rust trace or log as each step says, and compare every file of the folder with `files`, name by name and byte by byte, once `«now»` is put in the lines that have it. `daemon.json` holds the lines the daemon writes at its start and its stop with the time, pid, home, Node's version and memory named `$TIME`, `$PID`, `$HOME`, `$NODE` and `$MB`: Rust's words differ where Node's are its runtime's (`node v…`, a stack, `heap`), as `decision-36` says; the rest is the format.

<!-- file: daemon.json -->
```json
{
  "line": "{\"url\":\"http://127.0.0.1:$PORT/\",\"token\":\"$TOKEN\"}\n",
  "log": [
    "$TIME info start pid $PID node $NODE home $HOME",
    "$TIME info stop: stdin ended; rss $MB MB",
    "$TIME info exit 0"
  ]
}
```

The handle line is the first line the daemon prints for the app: JSON, key order `url` then `token`, the URL with a trailing slash, the token 48 lowercase hex digits (24 random bytes), and a line break. `$PORT` and `$TOKEN` stand for what varies.

<!-- file: operations.json -->
```json
{
  "operations": [
    "projects.list",
    "project.open",
    "project.resume",
    "project.close",
    "project.delete",
    "agents.list",
    "staff.last",
    "chief.switch",
    "member.add",
    "member.roles",
    "session.open",
    "session.hide",
    "session.end",
    "member.remove",
    "board.get",
    "inbox.get",
    "task.get",
    "task.transcript",
    "project.gate",
    "task.cancel",
    "task.pause",
    "task.reassign",
    "member.back",
    "task.resume",
    "tasks.delete",
    "message.read",
    "message.approve",
    "message.decline"
  ],
  "ping": "{\"ok\":true}"
}
```

## What differs between two recordings

Nothing, on one machine with one Node and the same checkout: every trace and file, recorded twice, is the same bytes (`node tests/goldens/daemon/record.mjs --check` records again and says where they are not; the recorder's own test does the same with a small program). What was named so that it is true: the ledger's path, the test's folder, the API's port, each token, the stamps a roster file and a trace take from the wall clock, the stack of a logged error; and the ledger's clock and names are steady (above). What would differ, and why it does not matter to a player:

- **On another Node.** The words of the runtime: `JSON.parse`'s messages and a `TypeError`'s in a 400 of the screens (`Expected property name or '}' in JSON at position 1 (line 1 column 2)`, `Cannot read properties of null (reading 'name')`). They are recorded as Node v26.8.1 wrote them; `decision-36` has the daemon speak Rust's own words where Node's were its runtime's, and the traces that carry them are `corners-screens-*`. Nothing else depends on the version.
- **On another platform.** The traces were recorded on macOS. A harness is found on `PATH` by its name there, and by its name with an extension (`.cmd`, `.exe`) on Windows (`harnessPath`), so the stand-ins in the `world` of the traces below, shell scripts named `claude`, `pi`… with no `.cmd` beside them, are not found by Node on Windows, and a Windows player would get another list of the harnesses installed (`harnesses` of `GET /api/agents`, `missing` of `agents.list`, the refusal of `chief.switch` for a harness not installed). The other traces do not depend on the platform. Whether these are run on POSIX only, or recorded again on the Windows leg with `.cmd` twins of their stand-ins, is for the lead to decide; the list is rewritten with the recording:

<!-- list: posix -->
```text
core-agents-server-001
core-agents-server-002
core-agents-server-003
core-agents-server-004
core-agents-server-005
core-agents-server-006
core-agents-server-007
core-agents-server-008
core-agents-server-009
corners-screens-001
corners-screens-002
corners-screens-003
corners-screens-004
corners-screens-005
corners-screens-006
corners-screens-007
corners-screens-008
corners-screens-009
```

- **After a release.** The catalog (`GET /api/agents`, `agents.list`), the version in `pages/agents.html`, the text `cf` prints (the traces of `cf` are what the checkout's `bin/cf` printed). A recording after a change to any of them differs in exactly what changed; that is what the tests that hold the checked-in files to what Node makes now catch.

## What is not recorded

- **The API's and the operations' own ledger calls**, one by one: only what they drew and logged, and what they left. A player cannot hold their order or arguments.
- **A test's reads of the ledger.**
- **HTTP framing:** headers other than `Authorization` and `Content-Type`, chunk boundaries, keep-alive. What Node answers is its status, type and bytes.
- **Time:** how long a long poll waited, how soon a close returned, how many times a poll asked the ledger. The long poll's wait and the pass loop are Rust tests on a paused clock (`decision-36`); the traces hold what the answer was.
- **A body that arrives after the state it was read against changed** (the stale views of `api.js`): a trace cannot say when a body's bytes came.
- **The harness admin's answers over HTTP.** `POST /api/harnesses/check` and `/update` are in the screens' traces where the screens decide (an unknown harness is a 400 before the admin is asked, `corners-screens-009`) and where the admin refuses with no clock in the answer (`update` of a harness that is not installed: `"Claude is not installed"`, a 400). Their answers at 200 are the harness admin's own goldens: the rows of `check` carry `checkedAt` from `Date.now()`, and `update` runs the harness's own updater. What the screens' worker has to hold them to: `check` at 200 is asserted by `tests/harness-admin.test.mjs:228-260` (`POST /api/harnesses/check` with `{}`, `harnessLatest` injected, the codex row's `update.state` `available`, no `integration` or `instructions` key in any row), `update` at 200 by no test over HTTP (the module's own is `harness-admin.test.mjs`), and the browser specs stand in for both routes with `page.route` (`app/tests/harnesses.spec.mjs:233, 276, 280`). The routes' wiring is `agents-server.js:109-127`: `check` asks `harnessAdmin.check(body.id ?? null, {refresh: body.refresh === true})`, `update` asks `harnessAdmin.update(body.id)`, and anything the admin throws is a 400 with its message.
- **The pass loop, the bridge, the stop, the process:** black-box tests and Rust tests, not traces.

## Where to look for what

| To hold | Look at |
|---|---|
| the order of the API's checks: 401 before 404, 403 before the body is read, the task before the verb | `corners-api-001`, `corners-api-004`, `core-api-001` |
| what the API takes for a window | `corners-api-002`, `core-api-002` |
| how it reads a target | `corners-api-003` |
| how it reads a body, bytes that are no UTF-8, a body over 2 MiB | `corners-api-004`, `corners-api-005` |
| how it reads and quotes numbers | `corners-api-006`, `corners-api-009`, `corners-api-010` |
| a window whose project is gone | `corners-api-007` |
| the door held open while the API closes | `core-daemon-001`; the hooks that wait for the board: `cf-board-023`, `024` |
| the dispatcher's wake-ups | `kicks` of every exchange and operation |
| the board as the dispatcher's projections merge into it | `corners-page-002`, `core-page-*` (`board.get`) |
| `project.resume`, the inbox, the staff a project opens with, the harnesses installed | `corners-page-001`, `003`, `004`, `006` |
| a refusal in the words of a string that was thrown | `corners-page-007` |
| the screens' token, the order of their checks, the body | `corners-screens-001` to `004` |
| the roster the screens write | `corners-screens-005`, `006`, `core-agents-server-006`, `007` |
| the roster that cannot be read | `corners-screens-007` |
| a harness that is not installed, asked to update | `corners-screens-010` |
| the pages | `core-agents-server-003`, `005`, `pages/` |
| the line formats and their rotation | `core-trace-002`, `core-log-001` |
