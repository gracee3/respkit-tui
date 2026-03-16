# Contributing

## Scope

`respkit-tui` is a keyboard-driven Rust frontend over the `respkit` Python ledger service.
Keep the boundary strict:
- Rust owns rendering, navigation, local UI state, and stdio JSON-RPC transport.
- Python owns ledger semantics, validation, approved-output derivation, and task-specific behavior.

Do not reimplement ledger or rename semantics in Rust.

## Development Setup

```bash
git clone <repo-url>
cd respkit-tui
cargo build
cargo test
```

For end-to-end work against the public SDK backend:

```bash
PYTHONPATH=/home/emmy/git/respkit python3 -m respkit.service.backend --ledger /path/to/ledger.sqlite --stdio
```

## Before Opening A Change

Run:

```bash
cargo fmt
cargo test
```

If you change protocol assumptions or UX behavior, update:
- `README.md`
- `docs/architecture.md`
- `docs/development.md` when workflow guidance changes
- `CHANGELOG.md`

## Design Rules

- Keep the backend protocol typed and explicit.
- Prefer small app-state transitions over implicit side effects.
- Preserve keyboard-first UX and visible discoverability of hotkeys.
- Surface backend gaps as unavailable; do not invent missing semantics in Rust.
- Treat adapter-provided fields and actions as generic data, not task-specific hardcoded logic.

## Pull Requests

Include:
- the user-facing behavior change
- protocol or config changes
- tests added or updated
- any known limitations that remain
