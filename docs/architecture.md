# Architecture

## Boundary

Rust owns:
- terminal rendering
- keyboard handling
- screen and modal state
- local queue/group filtering
- process management for the Python backend
- request/response bookkeeping

Python owns:
- ledger storage and semantics
- validation
- approved-output derivation
- action execution
- task adapter behavior
- preview generation
- category/risk/action metadata

## Runtime model

1. The startup screen collects a backend command, ledger path, and optional task.
2. `BackendClient` launches the backend with `bash -lc <rendered command>`.
3. The app sends line-delimited JSON-RPC requests over stdin.
4. A stdout reader thread parses JSON-RPC responses.
5. A stderr reader thread forwards log lines as notifications.
6. A wait thread reports backend exit so the UI can fall back to startup.
7. `App` keeps a `pending request id -> request kind` map and applies typed responses into screen state.

## Data flow

Initial connect:
- `ledger.health`
- `ledger.info`
- `ledger.tasks`
- `ledger.summary`
- `rows.list`
- `actions.list` when a concrete task is selected

Row detail:
- `rows.get`
- `rows.history`
- `rows.preview`

Mutations:
- `rows.validate` for approve-with-edit preflight
- `rows.decide` for row-level decisions
- `actions.invoke` for bulk/backend actions
- `export` for current queue snapshot

## UI model

Screens:
- startup
- dashboard
- queue
- groups
- detail
- bulk
- apply placeholder
- help

Modal types:
- single-line text input
- confirmation
- info/result

The app intentionally keeps filtering/grouping local after `rows.list` so v1 stays simple and responsive.
That is a UX/cache decision, not a ledger-logic reimplementation: the backend still defines row fields, categories, previews, actions, and decision validation.

## Current backend gaps surfaced in the UI

The public backend currently does not expose dedicated counts or plans for:
- missing source files
- path drift
- duplicate collisions
- final dry-run/apply plan

The TUI shows those as unavailable rather than inventing semantics in Rust.
