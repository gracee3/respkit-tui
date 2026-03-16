# Security Policy

## Reporting

Do not open public issues for security-sensitive problems.

Report security issues privately to the maintainer first and include:
- affected version or commit
- reproduction steps
- impact assessment
- any proposed mitigation

## Scope Notes

This repository is a local terminal UI client.
The highest-risk areas are:
- backend command execution and process launching
- JSON-RPC request/response parsing
- export paths and local filesystem writes
- rendering task-provided data safely without assuming structure

Task-specific validation and ledger semantics belong in the Python backend and its adapters.
Security fixes should preserve that boundary.
