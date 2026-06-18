# TODO

## Data Confidence

- Add provider checkout/qualification wrappers for exact-address confirmation. FCC/Esri block data is useful local evidence, but 100% address confidence should only be shown when a provider checkout or qualification API accepts the exact service address.
- Add a confidence model in the UI that distinguishes:
  - provider checkout confirmed
  - FCC/Fabric-derived likely availability
  - public pricing only
  - unknown/no local evidence
- Persist evidence fields from the backend through the frontend table so users can see why a provider is shown.

## Speeds

- Replace FCC served-threshold speed floors with maximum advertised speed data where available.
- Evaluate current FCC BDC nationwide downloads for provider/location speed fields and add an offline ingest path if the live API is not sufficient.
- Keep `BROADBANDMAP_API_KEY` support for max advertised speed enrichment where configured.
- Add speed source/basis display in the table, for example `FCC served floor`, `FCC max advertised`, `provider checkout`, or `public plan page`.

## Pricing

- Add provider-specific pricing parsers instead of relying only on generic dollar extraction.
- Add checkout-backed quote collection for major providers where technically and legally feasible.
- Expand public pricing source coverage for major national/regional ISPs:
  - Xfinity / Comcast
  - Spectrum / Charter
  - AT&T
  - Verizon
  - T-Mobile
  - Cox
  - Frontier
  - Optimum / Altice
  - Astound
  - Google Fiber
  - CenturyLink / Lumen
  - Brightspeed
  - EarthLink
  - Kinetic / Windstream
  - Mediacom
  - Metronet
  - Sparklight / Cable One
  - WOW!
  - Ziply Fiber
- Store pricing observations with timestamps, source URLs, parser version, and confidence.

## Performance

- Add durable backend caching for geocodes, FCC/Esri block lookups, and provider pricing observations.
- Move pricing warmups to a background worker queue with rate limits and retry/backoff.
- Add request timing/log fields for geocode, availability, pricing cache hit, and pricing warmup.
- Add stale-while-revalidate behavior for provider search results, not just address suggestions.

## Address Search

- Add a real local autocomplete provider, preferably Photon for a quick no-key path or Pelias for a stronger long-term stack.
- Add a separate address resolve endpoint so autocomplete suggestions and final Census geocoding are separate.
- Keep Census as final authoritative U.S. geocode/block lookup unless a paid address provider is configured.

## Frontend

- Make the results table occupy the full viewport width and height after search.
- Add sticky table header and pinned provider column.
- Surface confidence, evidence, and speed basis columns.
- Add loading states for background pricing refresh without resetting the whole page.
- Add table density controls and provider/type filters.

## Validation

- Add fixture tests for FCC/Esri block mapping, provider alias matching, speed basis, confidence, and pricing parser false positives.
- Add smoke tests for known addresses where FCC/Esri should return Spectrum, Xfinity, Verizon, T-Mobile, and local fiber providers.
- Add Playwright coverage once browser automation is available in the environment.
