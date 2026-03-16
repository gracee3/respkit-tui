# respkit-tui

Keyboard-driven Rust TUI for operating a `respkit` adjudication ledger through the Python service layer.

The Rust app owns rendering, keyboard navigation, local state, and stdio JSON-RPC transport.
The Python backend remains the source of truth for ledger semantics, validation, approved-output derivation, and action execution.

## Current v1 scope

Implemented:
- startup / connection screen
- dashboard with ledger counts and grouped summaries
- queue view with presets, filter, paging, and row inspector
- row detail with overview, preview, and history panels
- approve / approve-with-edit / reject / follow-up dispatch through `rows.decide`
- grouped category drill-down
- bulk action screen backed by `actions.list` / `actions.invoke`
- export of current queue scope to `.md`, `.csv`, or `.jsonl`
- simple local config with recent ledgers and default command/task
- stdio JSON-RPC backend launcher with stderr/protocol separation

Planned but not implemented because the public backend does not expose the RPC yet:
- final dry-run/apply plan
- backend-native missing-file/path-drift/collision counts
- bulk actions that require edit payloads
- task-specific structured edit forms beyond raw JSON

## Architecture

See [docs/architecture.md](docs/architecture.md).
Development workflow notes are in [docs/development.md](docs/development.md).

At a high level:
- `src/backend/client.rs`: launches the Python backend over `bash -lc`, sends JSON-RPC requests, reads stdout responses, and streams stderr/log/exit events separately.
- `src/backend/protocol.rs`: typed protocol envelopes and result payloads for the public `respkit` ledger service methods.
- `src/app.rs`: screen state, keyboard handling, pending request tracking, queue/group filtering, and action dispatch.
- `src/ui.rs`: `ratatui` rendering for each screen and modal.
- `src/config.rs`: config file load/save and recent-ledger handling.

## Build

```bash
cargo build
```

## Run

Default command if `~/git/respkit` exists:

```bash
cargo run
```

The startup screen pre-fills a backend command like:

```bash
PYTHONPATH=/home/emmy/git/respkit python -m respkit.service.backend --ledger {ledger} --stdio
```

You can replace it with any local backend command that speaks the same JSON-RPC protocol.
The command field supports:
- `{ledger}`: shell-quoted ledger path
- `{task}`: shell-quoted startup task value

If `{ledger}` is omitted, the app appends `--ledger <path>` automatically.
If `--stdio` is omitted, the app appends it automatically.

Example for a private task adapter:

```bash
PYTHONPATH=/home/emmy/git/respkit:/home/emmy/git/private-task \
python -m respkit.service.backend \
  --ledger {ledger} \
  --adapter private_pkg.service:PrivateTaskAdapter \
  --stdio
```

## Backend Protocol Assumptions

The TUI expects the public `respkit` stdio JSON-RPC backend documented in `~/git/respkit/README.md`.

Methods used by this TUI:
- `ledger.health`
- `ledger.info`
- `ledger.summary`
- `ledger.tasks`
- `rows.list`
- `rows.get`
- `rows.history`
- `rows.preview`
- `rows.validate`
- `rows.decide`
- `actions.list`
- `actions.invoke`
- `export`
- `system.shutdown`

Important assumptions:
- one JSON object per stdout line
- JSON-RPC `2.0`
- stderr is non-protocol log output
- adapter-provided `risk_flags`, `categories`, previews, validation, and actions are rendered generically
- the backend, not Rust, validates edits and derives approved output

## Screens And Keybindings

Global:
- `d`: dashboard
- `l`: queue
- `g`: groups
- `b`: bulk actions
- `p`: dry-run/apply placeholder
- `t`: cycle current task (`all` -> task1 -> task2 ...)
- `r`: refresh from backend
- `x`: export current queue scope
- `?`: help
- `q`: quit

Startup:
- `Tab` / `Shift-Tab`: move fields
- type: edit current field
- `Enter`: connect and load dashboard

Queue:
- `j` / `k` or arrows: move selection
- `PageUp` / `PageDown`: jump
- `[` / `]`: cycle queue preset
- `/`: set filter string
- `Backspace`: clear filter
- `Enter`: open row detail

Groups:
- `1`..`5`: choose grouping dimension
- `j` / `k`: move group selection
- `Enter`: drill selected group into queue view
- `c`: clear drill-down

Row detail:
- `o`: overview panel
- `p`: preview panel
- `h`: history panel
- `a`: approve
- `e`: approve with edit JSON
- `x`: reject
- `f`: mark needs follow-up / needs review
- `Esc`: back to queue

Bulk:
- `j` / `k`: move action selection
- `Enter`: invoke selected backend action on current queue scope

Apply:
- `Enter`: jump to `apply_ready` queue

## Config

Config file location:

```text
~/.config/respkit-tui/config.toml
```

Stored values:
- `backend_command`
- `default_ledger_path`
- `default_task_name`
- `recent_ledgers`

## Tests

Run:

```bash
cargo test
```

Coverage currently includes:
- RPC envelope parsing and error handling
- backend command rendering and a fake stdio backend round-trip
- config parsing / saving / recent ledger normalization
- queue preset and grouped drill-down state behavior
- export format inference

## Public Repo Hygiene

This repository includes:
- [LICENSE](LICENSE) for the project license
- [CONTRIBUTING.md](CONTRIBUTING.md) for change guidelines
- [SECURITY.md](SECURITY.md) for security reporting expectations
- [CHANGELOG.md](CHANGELOG.md) for release notes
- [.editorconfig](.editorconfig) for basic formatting consistency

## Known Limitations

- queue sorting is fixed to backend order
- search/filter is local over the fetched row set
- bulk actions requiring edits are listed but not executable
- export scope uses current visible row IDs; cross-task bulk/action/export flows work best when a concrete task is selected
- final rename/apply orchestration is a placeholder until the backend exposes a dedicated dry-run/apply RPC

## License

MIT. See [LICENSE](LICENSE).
