# Peregrinus

Peregrinus is a fast internet-provider search prototype inspired by instant search products. The
backend is `saker`, named after the saker falcon.

## Backend

```sh
cargo run -p saker
```

Default API:

- `GET /healthz`
- `POST /v1/search/address` with `{"address":"123 Main St, Denver, CO"}`

The current provider data is a deterministic seed catalog. Real provider availability, pricing,
and plan collection should be added behind `crates/saker-core`.

## Frontend

The Next.js frontend lives in `falco/`.

```sh
cd falco
bun install --frozen-lockfile
bun run dev
```

## Environment

Rust is pinned in `rust-toolchain.toml`. See `docs/agent-environment.md` for the Codex Cloud setup
script and required full-stack verification commands.
