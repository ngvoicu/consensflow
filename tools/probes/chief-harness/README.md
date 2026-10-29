# Probes of the chief-harness spec (CHF-02, CHF-03, CHF-10)

Replayable scripts that ask the real harnesses what the spec builds on. Each runs one
session at a time, in a throwaway config (never the owner's), strips every
`CONSENSFLOW_*` variable, says a login "set" and never shows it, hashes the owner's login
files before and after, and removes its throwaway. Run each with `--in <a folder under the
real home>` (Devin stalls in a workspace under the Mac's `/var` symlink).

| Script | What it answers | Costs |
|---|---|---|
| `models.py <harness> [--signed-out] [--record FILE]` | what each harness lists of its models, read the way ConsensFlow reads it; `--record` wrote the fixtures of `crates/cf-harness/tests/fixtures/models/` | no model turn |
| `resume_matrix.py <harness> [--a M] [--b M]` | T-445's resume rows, headless: start on A, resume with no flag, with `--model B`, with a model that does not exist, with no flag again; Devin also `--model <family>` | 5 or 6 one-line turns on the cheap models |
| `claude_alias_tail.py [alias\|tail]` | Claude's alias pinning in the record; the cost of a tail read of a 120 MB transcript | 3 one-line turns (alias) |
| `muse_model.py` (a Python with `pyte`; Mac) | Muse's `/model` in its window, and what its record says before and after a resume | 4 one-line turns on Muse Spark at minimal |
| `native_switch.py <harness> --ask <harness-ask>` (a Python with `pyte`; Mac) | CHF-10: the human's `/model` in the window with the key that keeps it to the conversation (Claude `s`, Codex `s`, Pi Enter, Devin `opt+↵`), then what ConsensFlow's own reader (`harness-ask` `last_used`) says, a headless reopen on it, and whether the switch wrote a model to the throwaway's settings | 3 one-line turns on the cheap models |
| `tuidrive.py` | T-445's pty driver (pyte), which `muse_model.py` and `native_switch.py` drive the windows with | |
| `moved_lines.py <repo> <file.md> <old> <new>` | CHF-01: each `file:line` a research or spec names, at the base it was read against and on another; which moved, by how much, and which changed | none |

## What they found (2026-10-28, the Mac)

Versions: Claude Code 2.1.312, Codex 0.165.0, Pi 1.4.0, Devin 3000.11.228, Muse Code 1.4.14.

### The record field that says the model in use, and what a resume does

| Harness | Field (CHF-05 reads) | Resume, no flag | Resume, `--model B` | Model that does not exist |
|---|---|---|---|---|
| Claude Code (`claude -p --resume`) | last non-sidechain assistant entry: `message.model` = `requestedModel` (the full id) and `effort` | keeps the model, **not the effort** (`low` → `medium`) | B wins | exit 1, `[claude-code:unrecognized_model] {"model":"claude-gone-9-9"}`; the record gets a `<synthetic>` assistant entry (`isApiErrorMessage`); the next no-flag resume runs on the last real model (the synthetic entry is skipped) |
| Codex (`codex exec resume`) | last `turn_context.payload.model` / `.effort` | **the account's default model (`gpt-6.1-sol`), not the session's** — unlike T-445's TUI resume, which kept both: the headless resume is no stand-in for the window's | B wins | exit 1: "The 'gpt-gone-9' model is not supported when using Codex with a ChatGPT account." (with "This session was recorded with model `gpt-6-luna` but is resuming with `gpt-gone-9`"); its `turn_context` names the gone model |
| Pi (`pi -p --session-id`) | last assistant `message`: `provider`, `model`, `thinkingLevel` | keeps model and thinking | B wins, thinking kept; no `model_change` written (as T-445 saw) | Pi warns "Model … not found for provider … Using custom model id", sends it, the provider answers 400 `model_not_found`; exit 1; an assistant message with `stopReason: error` names the gone model; **the next no-flag resume came up on the settings' default (`anthropic/claude-opus-4-8`)**, not the last good model: a `last_used` must skip error messages |
| Devin (`devin -r <id> -p`) | `sessions.model` | keeps it | B wins and rewrites `sessions.model` | refused at launch, before any turn: exit 1, "Error: Unknown model: 'swe-gone-9-medium'", then the families it has; `sessions.model` unchanged |
| Muse (`muse resume`, its window) | `run.model.configured.model_id` at each launch; a switch writes `runtime.model_reconfigure.completed {previous.model_id, effective.model_id}` at once, before any turn; the session index's `model_id` keeps the first model and is not to be read | keeps the switched model (`run.model.configured` = the new one); the effort is not restored (started `minimal`, the window says `max`) | (not run headless: `muse resume` takes no prompt) | `muse --model muse-gone-9 resume`: the window opens on it (`model_reconfigure.completed` old → muse-gone-9, `run.model.configured muse-gone-9`); the first turn fails at the provider, a `client` 404, "model `muse-gone-9` does not exist or you lack access", and the run ends `terminal: failed` (`model_failure_terminal_intent`) |

### The native switch

- **Muse's `/model`** opens "Choose model" (`muse-wonder-1.0`, `muse-spark-1.3`, `muse-spark-1.2` on this account), "↑↓ move · enter confirm · esc go back", then "Select Reasoning Level for <model>". **There is no session-only key: Enter saved `"model": "muse-spark-1.2"` to `settings.json`** (the throwaway's here). The help line of CHF-07 says so for Muse.
- A flag-driven change on resume (`--model`) also writes Muse's `model_reconfigure.completed`: MUS-24's "a model the harness moved to by itself" must compare it with the model the launch asked for, as the spec says.

### Claude's alias pinning

`--model sonnet` is recorded as `claude-sonnet-5-5` in both `message.model` and `requestedModel`, and a no-flag resume stays on it; `--model opus` likewise records `claude-opus-5-5`. The record pins the id an alias resolved to, so a reopen that passes the recorded id stays on it when the alias moves.

### The cost of a tail read (120 MB transcript, Python, the Mac under load)

| Last real assistant entry | Full read (parse every line) | Tail read, 64 KB blocks from the end, only lines naming an assistant parsed, 16 MB bound |
|---|---|---|
| 1 MB from the end | 0.9–1.7 s over 120 MB | 0.3 ms over 0.06 MB |
| 30 MB from the end (past the bound) | 0.8–1.1 s | 8 ms over 16 MB, then none: the reopen falls back to `participant.model` |

Parsing every line in the tail (a tool's output can be megabytes of JSON) cost 500–620 ms for the same 16 MB: CHF-05 should look for `"assistant"` in a line before parsing it.

### Devin's family without an effort (O2)

`devin -p --model swe-2` started a session whose `sessions.model` is `swe-2-high`: the first variant Devin lists for the family (`swe-2-high, swe-2-medium, swe-2-max`), not `medium`.

## zeewin2 (Windows 11, the same versions but Devin 3000.11.228 and Python 3.14.2)

`ssh zeewin2 'C:\Python314\python.exe -I - <harness> [flags]' < tools/probes/chief-harness/resume_matrix.py`

- **Codex:** every row as on the Mac (no-flag resume on the account's default `gpt-6.1-sol`; B wins; the gone model refused with the same words).
- **Pi** (`--a openai/gpt-5.6-luna --b openai/gpt-6-luna`: its `openai` provider there is a ChatGPT login, which serves only Codex's models, so `gpt-5-nano` and `gpt-5-mini` were refused 400 `model_not_found`): every row as on the Mac, the trap included: after the gone model, a no-flag resume came up on the settings' default (`openai/gpt-5.5`, which that login cannot run either).
- **Claude:** not run: the login there had 0.7 h left, and the probe refuses a login that expires within 2 h (a refresh in the copy could rotate the owner's token).
- **Devin:** not isolated, so the probe now refuses Devin on Windows. Devin finds its folders through Windows' known folders, not `APPDATA`: the run went on the owner's own login and store. The login file hashed the same before and after, but two of the probe's sessions are in the owner's `%APPDATA%\devin\cli\sessions.db` there (`wirehaired-hotel` on swe-2-medium, `phase-scourge` on swe-2-max, workspaces under `%TEMP%\cf-resume-matrix-*`), left for the owner to remove (`devin rm <id>`). What it showed: the gone model refused at launch with the Mac's words, and `--model swe-2` with no effort came up on **`swe-2-max`** there (`swe-2-high` in the Mac's empty config): the owner's own config there very probably picks the family's variant (`agent.preferred_family_models`; inferred, not read).

### The human's own switch, read by ConsensFlow (CHF-10, 2026-10-29, the Mac)

`native_switch.py`, the same versions. Each window opened on A, answered, was switched in
the window, answered on B; `harness-ask` `last_used` (the adapter's `last_used`) then said
B every time, and a headless reopen with what it said answered, the reader saying B again.
No switch wrote a model to the throwaway's settings; the owner's files were unchanged.

| Harness | A → B | The switch | `last_used` after | Written to the throwaway's settings |
|---|---|---|---|---|
| Claude Code | claude-sonnet-5-5 low → claude-haiku-5-5 | `/model`, Haiku, `s`; a conversation with a turn then asks "Switch model? … the full history gets re-read" (Yes) | `claude-haiku-5-5`, effort `low` | nothing |
| Codex | gpt-5.6-luna low → gpt-6-luna | `/model` (opens on the current model, the last row), GPT-6-Luna, Enter, then the reasoning list (on the model's default, medium) and `s` | `gpt-6-luna`, effort `medium` | `[tui]` flags only (`screen_reader_detection_done`, `model_availability_nux`) |
| Pi | openai-codex/gpt-5.6-luna minimal → openai-codex/gpt-6-luna | `/model`, typed `gpt-6-luna` (filters; another provider's model of the name can come first), Enter | `openai-codex/gpt-6-luna`, thinking `minimal` (an `openai/` pick reset it to `low`) | `lastChangelogVersion` only |
| Devin | swe-2-medium → swe-2-high | `/model` (opens on the family), `→`, `opt+↵` ("Model set to SWE-2 for this session") | `swe-2-high` | account and shell setup only, no `agent.model` |
