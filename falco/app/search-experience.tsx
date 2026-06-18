"use client";

import {
  FormEvent,
  KeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

type ServiceType =
  | "fiber"
  | "cable"
  | "fixed_wireless"
  | "satellite"
  | "dsl"
  | "unknown";

type Availability = "confirmed" | "likely" | "unknown";

type SearchSummary = {
  provider_count: number;
  cheapest_monthly_price_usd: number | null;
  fastest_downstream_mbps: number | null;
  priced_plan_count: number;
  pricing_observation_count: number;
  pricing_source_count: number;
  pricing_source_failure_count: number;
};

type GeocodedLocation = {
  latitude: number;
  longitude: number;
  census_block_geoid: string | null;
};

type AddressSuggestion = {
  address: string;
  latitude: number;
  longitude: number;
};

type InternetPlan = {
  name: string;
  downstream_mbps: number | null;
  upstream_mbps: number | null;
  monthly_price_usd: number | null;
  promo_months: number | null;
  equipment_fee_usd: number | null;
  install_fee_usd: number | null;
  data_cap_gb: number | null;
  contract_required: boolean | null;
  notes: string[];
};

type ProviderSource = {
  label: string;
  url: string | null;
};

type InternetProvider = {
  name: string;
  service_type: ServiceType;
  availability: Availability;
  headline: string;
  plans: InternetPlan[];
  source: ProviderSource;
  badges: string[];
  notes: string[];
};

type ProviderSearchResult = {
  address: string;
  matched_address: string;
  location: GeocodedLocation;
  summary: SearchSummary;
  providers: InternetProvider[];
  caveats: string[];
};

type SearchState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; result: ProviderSearchResult; elapsedMs: number }
  | { status: "error"; message: string };

type ProviderTableRow = {
  id: string;
  providerName: string;
  availability: Availability;
  serviceType: ServiceType;
  planCount: number;
  observedPrices: number[];
  monthlyRange: string;
  downstream: string;
  upstream: string;
  equipment: string;
  install: string;
  data: string;
  contract: string;
  sourceLabel: string;
  sourceUrl: string | null;
};

type CachedSuggestionPayload = {
  expiresAt: number;
  suggestions: AddressSuggestion[];
};

const apiBase =
  process.env.NEXT_PUBLIC_SAKER_API_URL?.replace(/\/$/, "") ??
  "http://127.0.0.1:1314";
const suggestionCacheTtlMs = 10 * 60 * 1000;
const staleSuggestionCacheTtlMs = 60 * 60 * 1000;

const currencyFormatter = new Intl.NumberFormat("en-US", {
  currency: "USD",
  maximumFractionDigits: 0,
  style: "currency",
});

const serviceLabels: Record<ServiceType, string> = {
  cable: "Cable",
  dsl: "DSL",
  fiber: "Fiber",
  fixed_wireless: "Fixed wireless",
  satellite: "Satellite",
  unknown: "Unknown",
};

const availabilityLabels: Record<Availability, string> = {
  confirmed: "Confirmed",
  likely: "Likely",
  unknown: "Unknown",
};

const availabilityClasses: Record<Availability, string> = {
  confirmed: "border-emerald-300 bg-emerald-50 text-emerald-800",
  likely: "border-sky-300 bg-sky-50 text-sky-800",
  unknown: "border-zinc-300 bg-zinc-50 text-zinc-700",
};

export default function SearchExperience() {
  const [address, setAddress] = useState("");
  const [state, setState] = useState<SearchState>({ status: "idle" });
  const [suggestions, setSuggestions] = useState<AddressSuggestion[]>([]);
  const [suggestionsOpen, setSuggestionsOpen] = useState(false);
  const [activeSuggestionIndex, setActiveSuggestionIndex] = useState(-1);
  const suggestionCacheRef = useRef<Map<string, CachedSuggestionPayload>>(
    new Map(),
  );
  const suggestionRequestIdRef = useRef(0);
  const pricingRefreshAttemptsRef = useRef<Map<string, number>>(new Map());

  const summaryCards = useMemo(() => {
    if (state.status !== "ready") {
      return [];
    }

    return [
      {
        label: "Providers",
        value: String(state.result.summary.provider_count),
      },
      {
        label: "Lowest price",
        value: formatPrice(state.result.summary.cheapest_monthly_price_usd),
      },
      {
        label: "Fastest download",
        value: formatMbps(state.result.summary.fastest_downstream_mbps),
      },
      {
        label: "Price intel",
        value: `${state.result.summary.pricing_observation_count}/${state.result.summary.pricing_source_count}`,
      },
    ];
  }, [state]);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    const trimmedAddress = address.trim();
    if (!trimmedAddress) {
      setState({ status: "error", message: "Address is required." });
      return;
    }

    pricingRefreshAttemptsRef.current.set(trimmedAddress, 0);
    await runSearch(trimmedAddress, { showLoading: true });
  }

  const runSearch = useCallback(async function runSearch(
    trimmedAddress: string,
    { showLoading }: { showLoading: boolean },
  ) {
    if (showLoading) {
      setState({ status: "loading" });
    }
    const startedAt = performance.now();

    try {
      const response = await fetch(`${apiBase}/v1/search/address`, {
        body: JSON.stringify({ address: trimmedAddress }),
        headers: { "content-type": "application/json" },
        method: "POST",
      });
      const payload = (await response.json()) as
        | ProviderSearchResult
        | { error?: string };

      if (!response.ok) {
        if (showLoading) {
          setState({
            status: "error",
            message:
              "error" in payload && payload.error
                ? payload.error
                : "Search failed.",
          });
        }
        return;
      }

      setState({
        status: "ready",
        result: payload as ProviderSearchResult,
        elapsedMs: performance.now() - startedAt,
      });
    } catch {
      if (showLoading) {
        setState({
          status: "error",
          message: `Could not reach saker at ${apiBase}.`,
        });
      }
    }
  }, []);

  useEffect(() => {
    if (state.status !== "ready") {
      return;
    }

    const { result } = state;
    const shouldRefreshPricing =
      result.summary.pricing_source_count > 0 &&
      result.summary.priced_plan_count === 0 &&
      result.summary.pricing_source_failure_count === 0;
    if (!shouldRefreshPricing) {
      return;
    }

    const attempts = pricingRefreshAttemptsRef.current.get(result.address) ?? 0;
    if (attempts >= 2) {
      return;
    }

    pricingRefreshAttemptsRef.current.set(result.address, attempts + 1);
    const timeout = window.setTimeout(() => {
      void runSearch(result.address, { showLoading: false });
    }, 1_800);

    return () => window.clearTimeout(timeout);
  }, [runSearch, state]);

  useEffect(() => {
    const query = address.trim();
    if (!shouldFetchAddressSuggestions(query)) {
      return;
    }

    const cacheKey = normalizeSuggestionQuery(query);
    const requestId = suggestionRequestIdRef.current + 1;
    suggestionRequestIdRef.current = requestId;
    const cachedSuggestions = readCachedSuggestions(
      cacheKey,
      suggestionCacheRef.current,
    );
    let cacheTimeout: number | undefined;
    if (cachedSuggestions) {
      cacheTimeout = window.setTimeout(() => {
        if (suggestionRequestIdRef.current !== requestId) {
          return;
        }
        setSuggestions(cachedSuggestions.suggestions);
        setSuggestionsOpen(cachedSuggestions.suggestions.length > 0);
        setActiveSuggestionIndex(
          cachedSuggestions.suggestions.length > 0 ? 0 : -1,
        );
      }, 0);

      if (!cachedSuggestions.stale) {
        return () => {
          if (cacheTimeout !== undefined) {
            window.clearTimeout(cacheTimeout);
          }
        };
      }
    }

    const controller = new AbortController();
    const timeout = window.setTimeout(async () => {
      try {
        const response = await fetch(
          `${apiBase}/v1/search/address/suggest?q=${encodeURIComponent(query)}`,
          { signal: controller.signal },
        );
        if (!response.ok) {
          return;
        }

        const payload = (await response.json()) as AddressSuggestion[];
        if (suggestionRequestIdRef.current !== requestId) {
          return;
        }

        writeCachedSuggestions(cacheKey, payload, suggestionCacheRef.current);
        setSuggestions(payload);
        setSuggestionsOpen(payload.length > 0);
        setActiveSuggestionIndex(payload.length > 0 ? 0 : -1);
      } catch (error) {
        if (!(error instanceof DOMException && error.name === "AbortError")) {
          setSuggestions([]);
          setSuggestionsOpen(false);
        }
      }
    }, 140);

    return () => {
      controller.abort();
      window.clearTimeout(timeout);
      if (cacheTimeout !== undefined) {
        window.clearTimeout(cacheTimeout);
      }
    };
  }, [address]);

  function handleAddressChange(nextAddress: string) {
    setAddress(nextAddress);
    if (!shouldFetchAddressSuggestions(nextAddress.trim())) {
      suggestionRequestIdRef.current += 1;
      setSuggestions([]);
      setSuggestionsOpen(false);
      setActiveSuggestionIndex(-1);
    }
  }

  function selectSuggestion(suggestion: AddressSuggestion) {
    setAddress(suggestion.address);
    storeRecentAddress(suggestion);
    setSuggestionsOpen(false);
    setActiveSuggestionIndex(-1);
  }

  function handleAddressKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (!suggestionsOpen || suggestions.length === 0) {
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveSuggestionIndex((currentIndex) =>
        currentIndex >= suggestions.length - 1 ? 0 : currentIndex + 1,
      );
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveSuggestionIndex((currentIndex) =>
        currentIndex <= 0 ? suggestions.length - 1 : currentIndex - 1,
      );
      return;
    }

    if (event.key === "Enter" && activeSuggestionIndex >= 0) {
      event.preventDefault();
      selectSuggestion(suggestions[activeSuggestionIndex]);
      return;
    }

    if (event.key === "Escape") {
      setSuggestionsOpen(false);
      setActiveSuggestionIndex(-1);
    }
  }

  return (
    <main className="min-h-screen bg-[#f7f8fb] text-[#111318]">
      <header className="border-b border-zinc-200 bg-white">
        <div className="mx-auto flex w-full max-w-6xl items-center justify-between px-4 py-4 sm:px-6">
          <div className="flex items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-md bg-[#111318] text-sm font-semibold text-white">
              P
            </div>
            <div>
              <p className="text-base font-semibold">Peregrinus</p>
              <p className="text-xs text-zinc-500">saker search engine</p>
            </div>
          </div>
          <div className="hidden items-center gap-2 text-xs text-zinc-500 sm:flex">
            <span className="h-2 w-2 rounded-md bg-emerald-500" />
            <span>{apiBase}</span>
          </div>
        </div>
      </header>

      <section className="border-b border-zinc-200 bg-white">
        <div className="mx-auto grid w-full max-w-6xl gap-8 px-4 py-10 sm:px-6 lg:grid-cols-[1.1fr_0.9fr] lg:py-14">
          <div className="flex flex-col justify-center">
            <p className="mb-3 text-sm font-medium text-[#2f6f62]">
              Internet options at an exact address
            </p>
            <h1 className="max-w-3xl text-4xl font-semibold leading-tight text-[#111318] sm:text-5xl">
              Peregrinus
            </h1>
            <form
              className="mt-8 flex w-full flex-col gap-3 sm:flex-row"
              onSubmit={handleSubmit}
            >
              <label className="sr-only" htmlFor="address">
                Address
              </label>
              <div className="relative min-w-0 flex-1">
                <input
                  aria-autocomplete="list"
                  aria-activedescendant={
                    suggestionsOpen && activeSuggestionIndex >= 0
                      ? suggestionOptionId(activeSuggestionIndex)
                      : undefined
                  }
                  aria-controls="address-suggestions"
                  aria-expanded={suggestionsOpen}
                  autoComplete="off"
                  className="h-12 w-full rounded-md border border-zinc-300 bg-white px-4 text-base text-[#111318] outline-none transition focus:border-[#2f6f62] focus:ring-4 focus:ring-emerald-100"
                  id="address"
                  onBlur={() => {
                    window.setTimeout(() => setSuggestionsOpen(false), 120);
                  }}
                  onChange={(event) => handleAddressChange(event.target.value)}
                  onFocus={() => setSuggestionsOpen(suggestions.length > 0)}
                  onKeyDown={handleAddressKeyDown}
                  placeholder="123 Main St, Denver, CO"
                  role="combobox"
                  type="search"
                  value={address}
                />
                {suggestionsOpen ? (
                  <div
                    className="absolute left-0 right-0 top-14 z-20 overflow-hidden rounded-md border border-zinc-200 bg-white shadow-lg"
                    id="address-suggestions"
                    role="listbox"
                  >
                    {suggestions.map((suggestion, index) => (
                      <button
                        className={`block w-full px-4 py-3 text-left text-sm ${
                          index === activeSuggestionIndex
                            ? "bg-emerald-50 text-[#111318]"
                            : "text-zinc-700 hover:bg-zinc-50"
                        }`}
                        aria-selected={index === activeSuggestionIndex}
                        id={suggestionOptionId(index)}
                        key={`${suggestion.address}-${suggestion.latitude}-${suggestion.longitude}`}
                        onMouseDown={(event) => {
                          event.preventDefault();
                          selectSuggestion(suggestion);
                        }}
                        role="option"
                        type="button"
                      >
                        <span className="block font-medium">
                          {suggestion.address}
                        </span>
                        <span className="mt-1 block text-xs text-zinc-500">
                          {suggestion.latitude.toFixed(5)},{" "}
                          {suggestion.longitude.toFixed(5)}
                        </span>
                      </button>
                    ))}
                  </div>
                ) : null}
              </div>
              <button
                className="h-12 rounded-md bg-[#111318] px-6 text-base font-semibold text-white transition hover:bg-[#263238] disabled:cursor-not-allowed disabled:bg-zinc-400"
                disabled={state.status === "loading"}
                type="submit"
              >
                {state.status === "loading" ? "Searching" : "Search"}
              </button>
            </form>
            <div aria-live="polite" className="mt-4 min-h-6 text-sm">
              {state.status === "error" ? (
                <p className="text-red-700">{state.message}</p>
              ) : null}
              {state.status === "ready" ? (
                <p className="text-zinc-600">
                  Matched {state.result.matched_address} in{" "}
                  {Math.round(state.elapsedMs)} ms
                </p>
              ) : null}
            </div>
          </div>

          <div className="grid min-h-[220px] grid-cols-2 gap-3 rounded-md border border-zinc-200 bg-[#f7f8fb] p-3">
            {summaryCards.length > 0 ? (
              summaryCards.map((card) => (
                <div
                  className="rounded-md border border-zinc-200 bg-white p-4"
                  key={card.label}
                >
                  <p className="text-xs font-medium uppercase text-zinc-500">
                    {card.label}
                  </p>
                  <p className="mt-3 text-2xl font-semibold text-[#111318]">
                    {card.value}
                  </p>
                </div>
              ))
            ) : (
              <div className="col-span-2 flex min-h-[190px] items-center justify-center rounded-md border border-dashed border-zinc-300 bg-white p-6 text-center text-sm text-zinc-500">
                Results snapshot
              </div>
            )}
          </div>
        </div>
      </section>

      <section className="mx-auto w-full max-w-6xl px-4 py-8 sm:px-6">
        {state.status === "ready" ? (
          <div className="grid gap-4">
            <LocationStrip result={state.result} />
            <ResultsTable result={state.result} />
          </div>
        ) : (
          <div className="grid gap-4 md:grid-cols-3">
            {["Fiber", "Cable", "Fixed wireless"].map((label) => (
              <div
                className="min-h-[120px] rounded-md border border-zinc-200 bg-white p-5"
                key={label}
              >
                <p className="text-sm font-semibold text-zinc-700">{label}</p>
                <div className="mt-5 h-3 w-24 rounded-md bg-zinc-200" />
                <div className="mt-3 h-3 w-32 rounded-md bg-zinc-100" />
              </div>
            ))}
          </div>
        )}
      </section>
    </main>
  );
}

function LocationStrip({ result }: { result: ProviderSearchResult }) {
  return (
    <div className="grid gap-3 rounded-md border border-zinc-200 bg-white p-4 text-sm text-zinc-600 md:grid-cols-3">
      <div>
        <p className="text-xs font-medium uppercase text-zinc-500">Matched</p>
        <p className="mt-1 font-medium text-zinc-800">{result.matched_address}</p>
      </div>
      <div>
        <p className="text-xs font-medium uppercase text-zinc-500">Coordinates</p>
        <p className="mt-1 font-medium text-zinc-800">
          {result.location.latitude.toFixed(5)},{" "}
          {result.location.longitude.toFixed(5)}
        </p>
      </div>
      <div>
        <p className="text-xs font-medium uppercase text-zinc-500">
          Census block
        </p>
        <p className="mt-1 font-medium text-zinc-800">
          {result.location.census_block_geoid ?? "Check"}
        </p>
      </div>
    </div>
  );
}

function ResultsTable({ result }: { result: ProviderSearchResult }) {
  const rows = buildProviderRows(result);

  if (rows.length === 0) {
    return (
      <div className="rounded-md border border-zinc-200 bg-white p-6 text-sm text-zinc-600">
        No provider rows were returned for this search.
      </div>
    );
  }

  return (
    <div className="overflow-x-auto rounded-md border border-zinc-200 bg-white shadow-sm">
      <table className="w-full min-w-[1180px] border-collapse text-left text-sm">
        <thead className="bg-zinc-50 text-xs font-semibold uppercase text-zinc-500">
          <tr>
            <TableHeader>Provider</TableHeader>
            <TableHeader>Availability</TableHeader>
            <TableHeader>Type</TableHeader>
            <TableHeader>Observed prices</TableHeader>
            <TableHeader>Down</TableHeader>
            <TableHeader>Up</TableHeader>
            <TableHeader>Equipment</TableHeader>
            <TableHeader>Install</TableHeader>
            <TableHeader>Data</TableHeader>
            <TableHeader>Contract</TableHeader>
            <TableHeader>Source</TableHeader>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-100">
          {rows.map((row) => (
            <tr className="align-top hover:bg-zinc-50" key={row.id}>
              <TableCell>
                <p className="font-semibold text-[#111318]">{row.providerName}</p>
              </TableCell>
              <TableCell>
                <span
                  className={`inline-flex rounded-md border px-2 py-1 text-xs font-semibold ${availabilityClasses[row.availability]}`}
                >
                  {availabilityLabels[row.availability]}
                </span>
              </TableCell>
              <TableCell>{serviceLabels[row.serviceType]}</TableCell>
              <TableCell>
                <p className="font-semibold text-[#111318]">{row.monthlyRange}</p>
                <p className="mt-1 text-xs text-zinc-500">
                  {row.planCount} observation{row.planCount === 1 ? "" : "s"}
                </p>
                <div className="mt-2 flex max-w-[320px] flex-wrap gap-1">
                  {row.observedPrices.slice(0, 8).map((price) => (
                    <span
                      className="rounded-md bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-700"
                      key={`${row.id}-${price}`}
                    >
                      {formatPrice(price)}
                    </span>
                  ))}
                  {row.observedPrices.length > 8 ? (
                    <span className="rounded-md bg-zinc-100 px-2 py-1 text-xs font-medium text-zinc-500">
                      +{row.observedPrices.length - 8}
                    </span>
                  ) : null}
                </div>
              </TableCell>
              <TableCell>{row.downstream}</TableCell>
              <TableCell>{row.upstream}</TableCell>
              <TableCell>{row.equipment}</TableCell>
              <TableCell>{row.install}</TableCell>
              <TableCell>{row.data}</TableCell>
              <TableCell>{row.contract}</TableCell>
              <TableCell>
                {row.sourceUrl ? (
                  <a
                    className="font-medium text-[#2f6f62] hover:underline"
                    href={row.sourceUrl}
                    rel="noreferrer"
                    target="_blank"
                  >
                    {row.sourceLabel}
                  </a>
                ) : (
                  row.sourceLabel
                )}
              </TableCell>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TableHeader({ children }: { children: ReactNode }) {
  return <th className="px-4 py-3">{children}</th>;
}

function TableCell({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return <td className={`px-4 py-4 text-zinc-700 ${className}`}>{children}</td>;
}

function shouldFetchAddressSuggestions(query: string) {
  if (query.length < 3) {
    return false;
  }

  if (/^\d{5}$/.test(query)) {
    return true;
  }

  if (/^\d+\s+\S{2,}/.test(query)) {
    return true;
  }

  if (query.includes(",")) {
    return query.length >= 4;
  }

  return query.length >= 5;
}

function normalizeSuggestionQuery(query: string) {
  return query.trim().replace(/\s+/g, " ").toLocaleLowerCase();
}

function readCachedSuggestions(
  cacheKey: string,
  memoryCache: Map<string, CachedSuggestionPayload>,
) {
  const now = Date.now();
  const memoryPayload = memoryCache.get(cacheKey);
  if (memoryPayload && memoryPayload.expiresAt > now - staleSuggestionCacheTtlMs) {
    return {
      stale: memoryPayload.expiresAt <= now,
      suggestions: memoryPayload.suggestions,
    };
  }

  const storagePayload = readSessionSuggestionCache(cacheKey);
  if (storagePayload) {
    memoryCache.set(cacheKey, storagePayload);
    return {
      stale: storagePayload.expiresAt <= now,
      suggestions: storagePayload.suggestions,
    };
  }

  return null;
}

function writeCachedSuggestions(
  cacheKey: string,
  suggestions: AddressSuggestion[],
  memoryCache: Map<string, CachedSuggestionPayload>,
) {
  const payload = {
    expiresAt: Date.now() + suggestionCacheTtlMs,
    suggestions,
  };
  memoryCache.set(cacheKey, payload);
  writeSessionSuggestionCache(cacheKey, payload);
}

function readSessionSuggestionCache(cacheKey: string) {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    const rawPayload = window.sessionStorage.getItem(sessionSuggestionKey(cacheKey));
    if (!rawPayload) {
      return null;
    }

    const payload = JSON.parse(rawPayload) as CachedSuggestionPayload;
    if (payload.expiresAt <= Date.now() - staleSuggestionCacheTtlMs) {
      window.sessionStorage.removeItem(sessionSuggestionKey(cacheKey));
      return null;
    }

    return payload;
  } catch {
    return null;
  }
}

function writeSessionSuggestionCache(
  cacheKey: string,
  payload: CachedSuggestionPayload,
) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    window.sessionStorage.setItem(
      sessionSuggestionKey(cacheKey),
      JSON.stringify(payload),
    );
  } catch {
    // Ignore quota/privacy-mode failures; in-memory cache still works.
  }
}

function sessionSuggestionKey(cacheKey: string) {
  return `peregrinus:suggest:${cacheKey}`;
}

function storeRecentAddress(suggestion: AddressSuggestion) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    const rawPayload = window.localStorage.getItem("peregrinus:recent-addresses");
    const current = rawPayload
      ? (JSON.parse(rawPayload) as AddressSuggestion[])
      : [];
    const next = [
      suggestion,
      ...current.filter((item) => item.address !== suggestion.address),
    ].slice(0, 5);
    window.localStorage.setItem(
      "peregrinus:recent-addresses",
      JSON.stringify(next),
    );
  } catch {
    // Recent addresses are an optimization only.
  }
}

function suggestionOptionId(index: number) {
  return `address-suggestion-${index}`;
}

function buildProviderRows(result: ProviderSearchResult): ProviderTableRow[] {
  return result.providers
    .map((provider) => {
      const prices = uniqueNumbers(
        provider.plans.flatMap((plan) =>
          plan.monthly_price_usd === null ? [] : [plan.monthly_price_usd],
        ),
      );
      const downstreamSpeeds = uniqueNumbers(
        provider.plans.flatMap((plan) =>
          plan.downstream_mbps === null ? [] : [plan.downstream_mbps],
        ),
      );
      const upstreamSpeeds = uniqueNumbers(
        provider.plans.flatMap((plan) =>
          plan.upstream_mbps === null ? [] : [plan.upstream_mbps],
        ),
      );

      return {
        availability: provider.availability,
        contract: summarizeContracts(provider.plans),
        data: summarizeDataCaps(provider.plans),
        downstream: formatRange(downstreamSpeeds, formatMbps),
        equipment: formatRange(
          uniqueNumbers(
            provider.plans.flatMap((plan) =>
              plan.equipment_fee_usd === null ? [] : [plan.equipment_fee_usd],
            ),
          ),
          formatPrice,
        ),
        id: `${provider.name}-${provider.service_type}`,
        install: formatRange(
          uniqueNumbers(
            provider.plans.flatMap((plan) =>
              plan.install_fee_usd === null ? [] : [plan.install_fee_usd],
            ),
          ),
          formatPrice,
        ),
        monthlyRange: formatRange(prices, formatPrice),
        observedPrices: prices,
        planCount: provider.plans.length,
        providerName: provider.name,
        serviceType: provider.service_type,
        sourceLabel: provider.source.label,
        sourceUrl: sourceUrlForProvider(provider),
        upstream: formatRange(upstreamSpeeds, formatMbps),
      };
    })
    .sort(compareProviderRows);
}

function compareProviderRows(first: ProviderTableRow, second: ProviderTableRow) {
  const firstAvailability = availabilityRank(first.availability);
  const secondAvailability = availabilityRank(second.availability);
  if (firstAvailability !== secondAvailability) {
    return firstAvailability - secondAvailability;
  }

  const firstPrice = first.observedPrices[0] ?? Number.POSITIVE_INFINITY;
  const secondPrice = second.observedPrices[0] ?? Number.POSITIVE_INFINITY;
  if (firstPrice !== secondPrice) {
    return firstPrice - secondPrice;
  }

  return first.providerName.localeCompare(second.providerName);
}

function availabilityRank(availability: Availability) {
  if (availability === "confirmed") {
    return 0;
  }
  if (availability === "likely") {
    return 1;
  }
  return 2;
}

function sourceUrlForProvider(provider: InternetProvider) {
  const noteSource = provider.plans
    .flatMap((plan) => plan.notes)
    .find((note) => note.startsWith("Source: "))
    ?.replace("Source: ", "");

  return noteSource ?? provider.source.url;
}

function uniqueNumbers(values: number[]) {
  return Array.from(new Set(values)).sort((first, second) => first - second);
}

function formatRange(values: number[], formatter: (value: number) => string) {
  if (values.length === 0) {
    return "Check";
  }

  if (values.length === 1) {
    return formatter(values[0]);
  }

  return `${formatter(values[0])} - ${formatter(values[values.length - 1])}`;
}

function summarizeDataCaps(plans: InternetPlan[]) {
  const caps = uniqueNumbers(
    plans.flatMap((plan) => (plan.data_cap_gb === null ? [] : [plan.data_cap_gb])),
  );
  if (caps.length === 0) {
    return "Check";
  }

  if (caps.length === 1) {
    return `${caps[0]} GB`;
  }

  return `${caps[0]} - ${caps[caps.length - 1]} GB`;
}

function summarizeContracts(plans: InternetPlan[]) {
  const knownValues = plans.flatMap((plan) =>
    plan.contract_required === null ? [] : [plan.contract_required],
  );
  if (knownValues.length === 0) {
    return "Check";
  }

  const hasRequired = knownValues.some(Boolean);
  const hasNoContract = knownValues.some((value) => !value);
  if (hasRequired && hasNoContract) {
    return "Varies";
  }

  return hasRequired ? "Yes" : "No";
}

function formatPrice(value: number | null) {
  if (value === null) {
    return "Check";
  }

  if (value === 0) {
    return "$0";
  }

  return currencyFormatter.format(value);
}

function formatMbps(value: number | null) {
  if (value === null || value <= 0) {
    return "Check";
  }

  if (value >= 1_000) {
    return `${value / 1_000} Gbps`;
  }

  return `${value} Mbps`;
}
