# Agent Guide

## Project Shape

- `services/saker`: Rust HTTP backend for provider search.
- `crates/saker-core`: Search domain models and aggregation logic.
- `falco`: Next.js frontend webapp.

Keep provider-specific integrations out of HTTP handlers. Add or replace them behind
`ProviderAggregator` in `crates/saker-core` first, then expose stable API changes through
`services/saker`.

## Checks Before Handoff

Run these from the repo root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cd falco
bun install --frozen-lockfile
bun run lint
bun run build
```

For cloud agents, follow `docs/agent-environment.md`.
