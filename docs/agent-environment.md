# Agent Environment

This repo is a full-stack monorepo with a Next.js frontend in `falco/` and a Rust backend service
named `saker`.

## Cloud setup script

Use this as the Codex Cloud environment setup script:

```sh
rustup toolchain install 1.95.0 --profile minimal --component clippy rustfmt
cargo fetch --locked
cargo check --workspace --all-targets
cargo test --workspace --all-targets

cd falco
bun install --frozen-lockfile
bun run lint
bun run build
```

The toolchain is pinned in `rust-toolchain.toml` so local and cloud agents compile with the
same Rust version.

## Useful commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p saker
```

```sh
cd falco
bun install --frozen-lockfile
bun run lint
bun run build
bun run dev
```

The backend listens on `127.0.0.1:1314` by default.

```sh
curl http://127.0.0.1:1314/healthz
curl -X POST http://127.0.0.1:1314/v1/search/address \
  -H 'content-type: application/json' \
  -d '{"address":"123 Main St, Denver, CO"}'
```

Set `SAKER_HOST` and `SAKER_PORT` in the cloud environment if the runner needs a different bind
address.
