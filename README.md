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

Search aggregation runs through live source integrations behind `crates/saker-core`:

- U.S. Census Geocoder for address matching and coordinates.
- BroadbandMap.com API for FCC-derived residential provider, technology, and maximum advertised
  speed availability when `BROADBANDMAP_API_KEY` is set.
- Public provider marketing-page collectors for pricing intelligence.

Pricing intelligence scrapes public provider pages and is not an address-specific checkout quote.
Add extra pricing sources with `PEREGRINUS_PRICING_SOURCES` as semicolon-separated
`Provider|service_type|https://...` entries. Exact address availability still needs
`BROADBANDMAP_API_KEY`, FCC/Fabric ingest, provider checkout integrations, or a partner feed.

## Frontend

The Next.js frontend lives in `falco/`.

```sh
cd falco
bun install --frozen-lockfile
bun run dev
```

The frontend dev server listens on `http://localhost:1313` and calls the Rust API at
`http://127.0.0.1:1314` by default. Override the API target with
`NEXT_PUBLIC_SAKER_API_URL`.

## Environment

Rust is pinned in `rust-toolchain.toml`. See `docs/agent-environment.md` for the Codex Cloud setup
script and required full-stack verification commands.
