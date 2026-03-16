# Development

## Commands

Build:

```bash
cargo build
```

Format and test:

```bash
cargo fmt
cargo test
```

Run the TUI:

```bash
cargo run
```

## Working Against The Public SDK

Create a demo ledger with the SDK example:

```bash
PYTHONPATH=/home/emmy/git/respkit python3 /home/emmy/git/respkit/examples/demo_ledger.py \
  --repo /tmp/respkit_tui_demo_repo \
  --ledger /tmp/respkit_tui_demo.sqlite
```

Then launch the TUI and point it at:

```bash
PYTHONPATH=/home/emmy/git/respkit python3 -m respkit.service.backend --ledger {ledger} --stdio
```

Use `/tmp/respkit_tui_demo.sqlite` as the ledger path.

## Integration Notes

Current live RPC usage:
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

The UI currently keeps queue filtering and grouping local after fetching rows.
If row volume grows enough to make that a problem, add backend query/sort support first rather than moving business rules into Rust.

## Release Hygiene

Before tagging a release:
- run `cargo fmt` and `cargo test`
- verify the README run instructions still match the startup defaults
- verify the backend protocol assumptions against the current `respkit` README and service implementation
- update `CHANGELOG.md`
