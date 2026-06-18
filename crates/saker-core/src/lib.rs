use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CENSUS_GEOCODER_URL: &str =
    "https://geocoding.geo.census.gov/geocoder/geographies/onelineaddress";
const CENSUS_LOCATION_URL: &str =
    "https://geocoding.geo.census.gov/geocoder/locations/onelineaddress";
const BROADBAND_MAP_INTERNET_URL: &str = "https://broadbandmap.com/api/v1/location/internet";
const ARCGIS_BDC_BLOCK_RECORDS_URL: &str = "https://services8.arcgis.com/peDZJliSvYims39Q/arcgis/rest/services/FCC_Broadband_Data_Collection_December_2024_View/FeatureServer/7/query";
const PRICING_SOURCES_ENV: &str = "PEREGRINUS_PRICING_SOURCES";
const ADDRESS_SUGGESTION_CACHE_TTL: Duration = Duration::from_mins(10);
const GEOCODE_CACHE_TTL_SECS: u64 = 86_400;
const ARCGIS_BLOCK_CACHE_TTL_SECS: u64 = 86_400;
const PRICING_SOURCE_CACHE_TTL: Duration = Duration::from_mins(15);
const PRICING_SOURCE_NEGATIVE_CACHE_TTL: Duration = Duration::from_mins(1);
const PRICING_SOURCE_WARMUP_TTL: Duration = Duration::from_secs(30);
const PRICING_SCRAPE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AddressQuery {
    pub address: String,
}

impl AddressQuery {
    #[must_use]
    pub fn normalized(&self) -> String {
        self.address
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ProviderSearchResult {
    pub address: String,
    pub matched_address: String,
    pub location: GeocodedLocation,
    pub summary: SearchSummary,
    pub providers: Vec<InternetProvider>,
    pub caveats: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GeocodedLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub census_block_geoid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct AddressSuggestion {
    pub address: String,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SearchSummary {
    pub provider_count: usize,
    pub cheapest_monthly_price_usd: Option<f32>,
    pub fastest_downstream_mbps: Option<u32>,
    pub priced_plan_count: usize,
    pub pricing_observation_count: usize,
    pub pricing_source_count: usize,
    pub pricing_source_failure_count: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct InternetProvider {
    pub name: String,
    pub service_type: ServiceType,
    pub availability: Availability,
    pub availability_confidence: u8,
    pub availability_evidence: String,
    pub headline: String,
    pub plans: Vec<InternetPlan>,
    pub source: ProviderSource,
    pub badges: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceType {
    Fiber,
    Cable,
    FixedWireless,
    Satellite,
    Dsl,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Confirmed,
    Likely,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedBasis {
    ReportedMaximum,
    PublicPlan,
    FccServedMinimum,
    FccUnderservedMinimum,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct InternetPlan {
    pub name: String,
    pub downstream_mbps: Option<u32>,
    pub upstream_mbps: Option<u32>,
    pub speed_basis: SpeedBasis,
    pub monthly_price_usd: Option<f32>,
    pub promo_months: Option<u8>,
    pub equipment_fee_usd: Option<f32>,
    pub install_fee_usd: Option<f32>,
    pub data_cap_gb: Option<u32>,
    pub contract_required: Option<bool>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ProviderSource {
    pub label: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingSourceSpec {
    pub provider_name: String,
    pub service_type: ServiceType,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
struct PricingObservation {
    provider_name: String,
    service_type: ServiceType,
    plan_name: String,
    downstream_mbps: Option<u32>,
    monthly_price_usd: f32,
    source_url: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct PricingCollection {
    observations: Vec<PricingObservation>,
    source_count: usize,
    failure_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct PricingSourceResult {
    source_url: String,
    observations: Vec<PricingObservation>,
    failed: bool,
}

#[derive(Debug, Clone)]
struct CachedAddressSuggestions {
    suggestions: Vec<AddressSuggestion>,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct CachedGeocodedAddress {
    address: GeocodedAddress,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct CachedProviders {
    providers: Vec<InternetProvider>,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct CachedPricingSource {
    observations: Vec<PricingObservation>,
    failed: bool,
    expires_at: Instant,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SearchError {
    #[error("address must not be empty")]
    EmptyAddress,
    #[error("no Census geocoder match found for this address")]
    AddressNotFound,
    #[error("real provider data source is not configured; set BROADBANDMAP_API_KEY")]
    ProviderDataSourceMissing,
    #[error("{source_name} request failed: {message}")]
    Upstream {
        source_name: String,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct ProviderAggregator {
    client: reqwest::Client,
    broadband_map_api_key: Option<String>,
    pricing_sources: Vec<PricingSourceSpec>,
    address_suggestion_cache: Arc<Mutex<HashMap<String, CachedAddressSuggestions>>>,
    geocode_cache: Arc<Mutex<HashMap<String, CachedGeocodedAddress>>>,
    arcgis_block_cache: Arc<Mutex<HashMap<String, CachedProviders>>>,
    pricing_source_cache: Arc<Mutex<HashMap<String, CachedPricingSource>>>,
    pricing_source_warmups: Arc<Mutex<HashMap<String, Instant>>>,
}

impl Default for ProviderAggregator {
    fn default() -> Self {
        Self::from_env()
    }
}

impl ProviderAggregator {
    #[must_use]
    pub fn from_env() -> Self {
        Self::with_pricing_sources(
            env::var("BROADBANDMAP_API_KEY").ok(),
            pricing_sources_from_env(),
        )
    }

    #[must_use]
    pub fn new(broadband_map_api_key: Option<String>) -> Self {
        Self::with_pricing_sources(broadband_map_api_key, default_pricing_sources())
    }

    #[must_use]
    pub fn with_pricing_sources(
        broadband_map_api_key: Option<String>,
        pricing_sources: Vec<PricingSourceSpec>,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .http1_only()
                .user_agent("Peregrinus pricing intelligence/0.1")
                .build()
                .unwrap_or_else(|_error| reqwest::Client::new()),
            broadband_map_api_key: broadband_map_api_key.filter(|key| !key.trim().is_empty()),
            pricing_sources,
            address_suggestion_cache: Arc::default(),
            geocode_cache: Arc::default(),
            arcgis_block_cache: Arc::default(),
            pricing_source_cache: Arc::default(),
            pricing_source_warmups: Arc::default(),
        }
    }

    /// Search real availability sources for internet providers at an address.
    ///
    /// The current pipeline geocodes through the U.S. Census Geocoder and then queries
    /// BroadbandMap.com's FCC-derived availability API when configured. Pricing intelligence is
    /// collected separately from provider marketing pages and merged without claiming exact-address
    /// checkout confirmation.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::EmptyAddress`] for blank input, [`SearchError::AddressNotFound`]
    /// when Census cannot geocode the address, and [`SearchError::Upstream`] for source failures.
    pub async fn search(&self, query: &AddressQuery) -> Result<ProviderSearchResult, SearchError> {
        let address = query.normalized();
        if address.is_empty() {
            return Err(SearchError::EmptyAddress);
        }

        let geocoded_address = self.geocode_address(&address).await?;
        let mut caveats = Vec::new();
        let mut providers = if self.broadband_map_api_key.is_some() {
            let providers = self
                .search_broadband_map(&geocoded_address.location)
                .await?;
            caveats.push(
                "Local availability is FCC-derived and still requires provider checkout for exact eligibility."
                    .into(),
            );
            providers
        } else if let Some(census_block_geoid) =
            geocoded_address.location.census_block_geoid.as_deref()
        {
            let providers = self.search_arcgis_bdc_block(census_block_geoid).await;
            if providers.is_empty() {
                caveats.push(
                    "No FCC/Esri block-level provider records were returned for this Census block."
                        .into(),
                );
            } else {
                caveats.push(
                    "Using FCC/Esri block-level availability because BROADBANDMAP_API_KEY is not configured. This is local availability intelligence, not provider checkout confirmation."
                        .into(),
                );
            }
            providers
        } else {
            caveats.push(
                "Census did not return a Census block GEOID, so local provider availability could not be inferred."
                    .into(),
            );
            Vec::new()
        };
        let pricing_collection = self.collect_pricing_intelligence(&providers);

        merge_pricing_observations(&mut providers, &pricing_collection.observations);

        if pricing_collection.observations.is_empty() {
            caveats.push(
                "No public pricing observations were collected from the configured provider pages."
                    .into(),
            );
        } else {
            caveats.push(format!(
                "Pricing intelligence is scraped from public provider marketing pages and is not an address-specific checkout quote. {} observations from {} sources were collected.",
                pricing_collection.observations.len(),
                pricing_collection.source_count
            ));
        }

        if pricing_collection.failure_count > 0 {
            caveats.push(format!(
                "{} pricing source(s) failed or returned no parseable plan price during this search.",
                pricing_collection.failure_count
            ));
        }

        caveats.extend([
            "Provider checkout remains the source of truth for address eligibility, taxes, installation fees, equipment fees, autopay discounts, data caps, and contract terms.".into(),
            "FCC availability data reports providers, technology, and maximum advertised speeds when configured. It does not include retail price, promo, equipment, data-cap, or contract terms.".into(),
        ]);

        Ok(ProviderSearchResult {
            address,
            matched_address: geocoded_address.matched_address,
            location: geocoded_address.location,
            summary: summarize(&providers, &pricing_collection),
            providers,
            caveats,
        })
    }

    /// Return fast address suggestions backed by the U.S. Census location geocoder.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::EmptyAddress`] for blank input and [`SearchError::Upstream`] for
    /// source failures.
    pub async fn suggest_addresses(
        &self,
        query: &AddressQuery,
    ) -> Result<Vec<AddressSuggestion>, SearchError> {
        let address = query.normalized();
        if address.is_empty() {
            return Err(SearchError::EmptyAddress);
        }

        let cache_key = normalize_cache_key(&address);
        if let Some(cached_suggestions) = self.cached_address_suggestions(&cache_key) {
            return Ok(cached_suggestions);
        }

        let response = self
            .client
            .get(CENSUS_LOCATION_URL)
            .query(&[
                ("address", address.as_str()),
                ("benchmark", "Public_AR_Current"),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|error| upstream_error("Census Location Geocoder", &error))?;

        if !response.status().is_success() {
            return Err(SearchError::Upstream {
                source_name: "Census Location Geocoder".into(),
                message: response.status().to_string(),
            });
        }

        let geocode_response: CensusGeocoderResponse = response
            .json()
            .await
            .map_err(|error| upstream_error("Census Location Geocoder", &error))?;

        let suggestions: Vec<AddressSuggestion> = geocode_response
            .result
            .address_matches
            .into_iter()
            .take(6)
            .map(|matched_address| AddressSuggestion {
                address: matched_address.matched_address,
                latitude: matched_address.coordinates.y,
                longitude: matched_address.coordinates.x,
            })
            .collect();

        self.store_address_suggestions(cache_key, suggestions.clone());

        Ok(suggestions)
    }

    fn cached_address_suggestions(&self, cache_key: &str) -> Option<Vec<AddressSuggestion>> {
        let cache = self.address_suggestion_cache.lock().ok()?;
        let cached = cache.get(cache_key)?;
        if cached.expires_at <= Instant::now() {
            return None;
        }

        Some(cached.suggestions.clone())
    }

    fn store_address_suggestions(&self, cache_key: String, suggestions: Vec<AddressSuggestion>) {
        let Ok(mut cache) = self.address_suggestion_cache.lock() else {
            return;
        };

        cache.retain(|_key, cached| cached.expires_at > Instant::now());
        cache.insert(
            cache_key,
            CachedAddressSuggestions {
                suggestions,
                expires_at: Instant::now() + ADDRESS_SUGGESTION_CACHE_TTL,
            },
        );
    }

    async fn geocode_address(&self, address: &str) -> Result<GeocodedAddress, SearchError> {
        let cache_key = normalize_cache_key(address);
        if let Some(geocoded_address) = self.cached_geocoded_address(&cache_key) {
            return Ok(geocoded_address);
        }

        let response = self
            .client
            .get(CENSUS_GEOCODER_URL)
            .query(&[
                ("address", address),
                ("benchmark", "Public_AR_Current"),
                ("vintage", "Current_Current"),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|error| upstream_error("Census Geocoder", &error))?;

        if !response.status().is_success() {
            return Err(SearchError::Upstream {
                source_name: "Census Geocoder".into(),
                message: response.status().to_string(),
            });
        }

        let geocode_response: CensusGeocoderResponse = response
            .json()
            .await
            .map_err(|error| upstream_error("Census Geocoder", &error))?;

        let matched_address = geocode_response
            .result
            .address_matches
            .into_iter()
            .next()
            .ok_or(SearchError::AddressNotFound)?;

        let census_block_geoid = matched_address.census_block_geoid();

        let geocoded_address = GeocodedAddress {
            matched_address: matched_address.matched_address,
            location: GeocodedLocation {
                latitude: matched_address.coordinates.y,
                longitude: matched_address.coordinates.x,
                census_block_geoid,
            },
        };

        self.store_geocoded_address(cache_key, geocoded_address.clone());

        Ok(geocoded_address)
    }

    fn cached_geocoded_address(&self, cache_key: &str) -> Option<GeocodedAddress> {
        let cache = self.geocode_cache.lock().ok()?;
        let cached = cache.get(cache_key)?;
        if cached.expires_at <= Instant::now() {
            return None;
        }

        Some(cached.address.clone())
    }

    fn store_geocoded_address(&self, cache_key: String, address: GeocodedAddress) {
        let Ok(mut cache) = self.geocode_cache.lock() else {
            return;
        };

        cache.retain(|_key, cached| cached.expires_at > Instant::now());
        cache.insert(
            cache_key,
            CachedGeocodedAddress {
                address,
                expires_at: Instant::now() + Duration::from_secs(GEOCODE_CACHE_TTL_SECS),
            },
        );
    }

    async fn search_broadband_map(
        &self,
        location: &GeocodedLocation,
    ) -> Result<Vec<InternetProvider>, SearchError> {
        let api_key = self
            .broadband_map_api_key
            .as_deref()
            .ok_or(SearchError::ProviderDataSourceMissing)?;

        let response = self
            .client
            .get(BROADBAND_MAP_INTERNET_URL)
            .bearer_auth(api_key)
            .query(&[
                ("lat", location.latitude.to_string()),
                ("lng", location.longitude.to_string()),
                ("service_type", "residential".into()),
            ])
            .send()
            .await
            .map_err(|error| upstream_error("BroadbandMap.com", &error))?;

        if !response.status().is_success() {
            return Err(SearchError::Upstream {
                source_name: "BroadbandMap.com".into(),
                message: response.status().to_string(),
            });
        }

        let broadband_response: BroadbandMapInternetResponse = response
            .json()
            .await
            .map_err(|error| upstream_error("BroadbandMap.com", &error))?;

        Ok(broadband_response
            .providers
            .iter()
            .map(map_broadband_map_provider)
            .collect())
    }

    async fn search_arcgis_bdc_block(&self, census_block_geoid: &str) -> Vec<InternetProvider> {
        if let Some(providers) = self.cached_arcgis_block(census_block_geoid) {
            return providers;
        }

        let where_clause = format!("GEOID = '{}'", census_block_geoid.replace('\'', "''"));
        let response = self
            .client
            .get(ARCGIS_BDC_BLOCK_RECORDS_URL)
            .query(&[
                ("f", "json"),
                ("where", where_clause.as_str()),
                (
                    "outFields",
                    "ProviderName,FRN,Technology,TotalBSLs,ServedBSLs,UnderservedBSLs,UnservedBSLs",
                ),
                ("returnGeometry", "false"),
                ("resultRecordCount", "100"),
            ])
            .send()
            .await;

        let Ok(response) = response else {
            return Vec::new();
        };
        if !response.status().is_success() {
            return Vec::new();
        }

        let providers = response
            .json::<ArcgisQueryResponse>()
            .await
            .map(|arcgis_response| map_arcgis_bdc_records(&arcgis_response.features))
            .unwrap_or_default();

        self.store_arcgis_block(census_block_geoid, providers.clone());

        providers
    }

    fn cached_arcgis_block(&self, census_block_geoid: &str) -> Option<Vec<InternetProvider>> {
        let cache = self.arcgis_block_cache.lock().ok()?;
        let cached = cache.get(census_block_geoid)?;
        if cached.expires_at <= Instant::now() {
            return None;
        }

        Some(cached.providers.clone())
    }

    fn store_arcgis_block(&self, census_block_geoid: &str, providers: Vec<InternetProvider>) {
        let Ok(mut cache) = self.arcgis_block_cache.lock() else {
            return;
        };

        cache.retain(|_key, cached| cached.expires_at > Instant::now());
        cache.insert(
            census_block_geoid.into(),
            CachedProviders {
                providers,
                expires_at: Instant::now() + Duration::from_secs(ARCGIS_BLOCK_CACHE_TTL_SECS),
            },
        );
    }

    fn collect_pricing_intelligence(&self, providers: &[InternetProvider]) -> PricingCollection {
        let pricing_sources = pricing_sources_for_providers(&self.pricing_sources, providers);
        let mut collection = PricingCollection {
            source_count: pricing_sources.len(),
            ..PricingCollection::default()
        };

        for source in pricing_sources {
            if let Some(cached_source) = self.cached_pricing_source(&source.url) {
                if cached_source.failed || cached_source.observations.is_empty() {
                    collection.failure_count += 1;
                } else {
                    collection.observations.extend(cached_source.observations);
                }
                continue;
            }

            self.spawn_pricing_warmup(source);
        }

        collection
    }

    fn cached_pricing_source(&self, source_url: &str) -> Option<CachedPricingSource> {
        let cache = self.pricing_source_cache.lock().ok()?;
        let cached = cache.get(source_url)?;
        if cached.expires_at <= Instant::now() {
            return None;
        }

        Some(cached.clone())
    }

    fn spawn_pricing_warmup(&self, source: PricingSourceSpec) {
        if !self.reserve_pricing_warmup(&source.url) {
            return;
        }

        let client = self.client.clone();
        let pricing_source_cache = Arc::clone(&self.pricing_source_cache);
        let pricing_source_warmups = Arc::clone(&self.pricing_source_warmups);
        let source_url = source.url.clone();

        tokio::spawn(async move {
            let source_result = match scrape_pricing_source(client, source).await {
                Ok(source_result) | Err(source_result) => source_result,
            };
            store_pricing_source_result(&pricing_source_cache, source_result);
            if let Ok(mut warmups) = pricing_source_warmups.lock() {
                warmups.remove(&source_url);
            }
        });
    }

    fn reserve_pricing_warmup(&self, source_url: &str) -> bool {
        let Ok(mut warmups) = self.pricing_source_warmups.lock() else {
            return false;
        };
        let now = Instant::now();
        warmups.retain(|_source_url, expires_at| *expires_at > now);
        if warmups.contains_key(source_url) {
            return false;
        }

        warmups.insert(source_url.into(), now + PRICING_SOURCE_WARMUP_TTL);
        true
    }
}

fn store_pricing_source_result(
    pricing_source_cache: &Arc<Mutex<HashMap<String, CachedPricingSource>>>,
    source_result: PricingSourceResult,
) {
    let Ok(mut cache) = pricing_source_cache.lock() else {
        return;
    };

    cache.retain(|_source_url, cached| cached.expires_at > Instant::now());
    cache.insert(
        source_result.source_url,
        CachedPricingSource {
            observations: source_result.observations,
            failed: source_result.failed,
            expires_at: Instant::now()
                + if source_result.failed {
                    PRICING_SOURCE_NEGATIVE_CACHE_TTL
                } else {
                    PRICING_SOURCE_CACHE_TTL
                },
        },
    );
}

fn summarize(
    providers: &[InternetProvider],
    pricing_collection: &PricingCollection,
) -> SearchSummary {
    let plan_prices = providers
        .iter()
        .flat_map(|provider| provider.plans.iter())
        .filter_map(|plan| plan.monthly_price_usd);

    let downstream_speeds = providers
        .iter()
        .flat_map(|provider| provider.plans.iter())
        .filter_map(|plan| plan.downstream_mbps);

    let priced_plan_count = providers
        .iter()
        .flat_map(|provider| provider.plans.iter())
        .filter(|plan| plan.monthly_price_usd.is_some())
        .count();

    SearchSummary {
        provider_count: providers.len(),
        cheapest_monthly_price_usd: plan_prices.min_by(f32::total_cmp),
        fastest_downstream_mbps: downstream_speeds.max(),
        priced_plan_count,
        pricing_observation_count: pricing_collection.observations.len(),
        pricing_source_count: pricing_collection.source_count,
        pricing_source_failure_count: pricing_collection.failure_count,
    }
}

fn map_arcgis_bdc_records(features: &[ArcgisFeature]) -> Vec<InternetProvider> {
    let mut providers = Vec::new();

    for feature in features {
        let provider = map_arcgis_bdc_record(&feature.attributes);
        merge_provider(&mut providers, provider);
    }

    providers
}

fn fcc_speed_floor(
    served_bsls: u32,
    underserved_bsls: u32,
) -> (Option<u32>, Option<u32>, SpeedBasis, String) {
    if served_bsls > 0 {
        return (
            Some(100),
            Some(20),
            SpeedBasis::FccServedMinimum,
            "FCC BDC served classification implies at least 100 Mbps down / 20 Mbps up for served BSLs in this block record.".into(),
        );
    }

    if underserved_bsls > 0 {
        return (
            Some(25),
            Some(3),
            SpeedBasis::FccUnderservedMinimum,
            "FCC BDC underserved classification implies at least 25 Mbps down / 3 Mbps up but below served threshold for BSLs in this block record.".into(),
        );
    }

    (
        None,
        None,
        SpeedBasis::Unknown,
        "FCC BDC block record did not report served or underserved BSLs for this provider/technology.".into(),
    )
}

fn map_arcgis_bdc_record(record: &ArcgisBdcRecord) -> InternetProvider {
    let service_type = service_type_from_technology_code(record.technology);
    let technology_label = technology_label_from_code(record.technology);
    let service_label = service_label(&service_type);
    let availability = if record.served_bsls > 0 || record.underserved_bsls > 0 {
        Availability::Likely
    } else {
        Availability::Unknown
    };
    let (downstream_mbps, upstream_mbps, speed_basis, speed_note) =
        fcc_speed_floor(record.served_bsls, record.underserved_bsls);
    let availability_confidence =
        if record.served_bsls == record.total_bsls && record.total_bsls > 0 {
            92
        } else if record.served_bsls > 0 || record.underserved_bsls > 0 {
            84
        } else {
            45
        };

    InternetProvider {
        name: normalize_display_provider_name(&record.provider_name),
        service_type,
        availability,
        availability_confidence,
        availability_evidence: "FCC BDC block-level provider report".into(),
        headline: format!(
            "{} has FCC BDC block-level {} records in this Census block.",
            record.provider_name, technology_label
        ),
        plans: vec![InternetPlan {
            name: format!("FCC block-level {service_label} availability record"),
            downstream_mbps,
            upstream_mbps,
            speed_basis,
            monthly_price_usd: None,
            promo_months: None,
            equipment_fee_usd: None,
            install_fee_usd: None,
            data_cap_gb: None,
            contract_required: None,
            notes: vec![
                speed_note,
                format!(
                    "FCC BDC block summary: {} served, {} underserved, {} unserved out of {} BSLs.",
                    record.served_bsls,
                    record.underserved_bsls,
                    record.unserved_bsls,
                    record.total_bsls
                ),
                "The FCC/Esri block table confirms local provider/technology presence but does not include retail plan prices or exact checkout eligibility.".into(),
            ],
        }],
        source: ProviderSource {
            label: "FCC BDC via ArcGIS Living Atlas".into(),
            url: Some("https://www.arcgis.com/home/item.html?id=e1343efcefc344709057260ee57290a0".into()),
        },
        badges: vec![
            "FCC BDC".into(),
            service_label.to_string(),
            technology_label.to_string(),
        ],
        notes: vec![
            "Availability is likely, not confirmed, until the provider accepts the exact service address.".into(),
        ],
    }
}

fn merge_provider(providers: &mut Vec<InternetProvider>, incoming_provider: InternetProvider) {
    if let Some(existing_provider) = providers
        .iter_mut()
        .find(|provider| provider_names_match(&provider.name, &incoming_provider.name))
    {
        existing_provider.service_type = best_service_type(
            &existing_provider.service_type,
            &incoming_provider.service_type,
        );
        existing_provider.availability = best_availability(
            &existing_provider.availability,
            &incoming_provider.availability,
        );
        if incoming_provider.availability_confidence > existing_provider.availability_confidence {
            existing_provider.availability_confidence = incoming_provider.availability_confidence;
            existing_provider.availability_evidence = incoming_provider.availability_evidence;
        }
        existing_provider.plans.extend(incoming_provider.plans);
        existing_provider.badges.extend(incoming_provider.badges);
        existing_provider.notes.extend(incoming_provider.notes);
        existing_provider.badges.sort();
        existing_provider.badges.dedup();
        existing_provider.notes.sort();
        existing_provider.notes.dedup();
    } else {
        providers.push(incoming_provider);
    }
}

fn map_broadband_map_provider(provider: &BroadbandMapProvider) -> InternetProvider {
    let service_type = service_type_from_technology(&provider.technology);
    let service_label = service_label(&service_type);
    let plan_name = format!("Reported maximum {service_label} offering");

    InternetProvider {
        name: provider.name.clone(),
        service_type,
        availability: Availability::Likely,
        availability_confidence: 90,
        availability_evidence: "BroadbandMap.com FCC-derived location lookup".into(),
        headline: format!(
            "{} reports residential {} availability up to {} down / {} up.",
            provider.name,
            provider.technology,
            format_mbps(provider.max_download_mbps),
            format_mbps(provider.max_upload_mbps)
        ),
        plans: vec![InternetPlan {
            name: plan_name,
            downstream_mbps: Some(provider.max_download_mbps),
            upstream_mbps: Some(provider.max_upload_mbps),
            speed_basis: SpeedBasis::ReportedMaximum,
            monthly_price_usd: None,
            promo_months: None,
            equipment_fee_usd: None,
            install_fee_usd: None,
            data_cap_gb: None,
            contract_required: None,
            notes: vec![
                "This is the provider's maximum advertised speed in the source data, not a retail plan quote.".into(),
                "Pricing and terms require provider checkout or a partner pricing feed.".into(),
            ],
        }],
        source: ProviderSource {
            label: "BroadbandMap.com API".into(),
            url: Some("https://broadbandmap.com/developers/".into()),
        },
        badges: badges_for_service_type(&provider.technology),
        notes: vec![
            "Availability is likely, not confirmed, until the provider accepts the exact service address.".into(),
        ],
    }
}

fn merge_pricing_observations(
    providers: &mut Vec<InternetProvider>,
    observations: &[PricingObservation],
) {
    for observation in observations {
        let plan = pricing_observation_plan(observation);

        if let Some(provider) = providers
            .iter_mut()
            .find(|provider| provider_names_match(&provider.name, &observation.provider_name))
        {
            provider.plans.push(plan);
            provider.badges.push("public price observed".into());
            provider
                .notes
                .push("Public pricing was collected separately from provider availability.".into());
        } else {
            providers.push(InternetProvider {
                name: observation.provider_name.clone(),
                service_type: observation.service_type.clone(),
                availability: Availability::Unknown,
                availability_confidence: 30,
                availability_evidence: "Public pricing page only; no local availability source matched".into(),
                headline:
                    "Public pricing observed; exact address availability still needs checkout or FCC/Fabric matching."
                        .into(),
                plans: vec![plan],
                source: ProviderSource {
                    label: "Public pricing scrape".into(),
                    url: Some(observation.source_url.clone()),
                },
                badges: vec!["public price observed".into()],
                notes: vec![
                    "This provider was added from pricing intelligence, not confirmed address availability.".into(),
                ],
            });
        }
    }

    for provider in providers {
        provider.badges.sort();
        provider.badges.dedup();
        provider.notes.sort();
        provider.notes.dedup();
    }
}

fn pricing_observation_plan(observation: &PricingObservation) -> InternetPlan {
    InternetPlan {
        name: observation.plan_name.clone(),
        downstream_mbps: observation.downstream_mbps,
        upstream_mbps: None,
        speed_basis: observation
            .downstream_mbps
            .map_or(SpeedBasis::Unknown, |_speed| SpeedBasis::PublicPlan),
        monthly_price_usd: Some(observation.monthly_price_usd),
        promo_months: None,
        equipment_fee_usd: None,
        install_fee_usd: None,
        data_cap_gb: None,
        contract_required: None,
        notes: vec![
            "Scraped from a public marketing page; not an address-specific checkout quote.".into(),
            format!("Source: {}", observation.source_url),
        ],
    }
}

async fn scrape_pricing_source(
    client: reqwest::Client,
    source: PricingSourceSpec,
) -> Result<PricingSourceResult, PricingSourceResult> {
    let source_url = source.url.clone();
    let response = client
        .get(&source.url)
        .timeout(PRICING_SCRAPE_TIMEOUT)
        .send()
        .await
        .map_err(|_error| failed_pricing_source(source_url.clone()))?;

    if !response.status().is_success() {
        return Err(failed_pricing_source(source_url));
    }

    let body = response
        .text()
        .await
        .map_err(|_error| failed_pricing_source(source_url.clone()))?;

    let observations = extract_pricing_observations(&source, &body);

    Ok(PricingSourceResult {
        source_url,
        failed: observations.is_empty(),
        observations,
    })
}

fn failed_pricing_source(source_url: String) -> PricingSourceResult {
    PricingSourceResult {
        source_url,
        observations: Vec::new(),
        failed: true,
    }
}

fn extract_pricing_observations(source: &PricingSourceSpec, body: &str) -> Vec<PricingObservation> {
    if matches!(
        source.service_type,
        ServiceType::FixedWireless | ServiceType::Satellite
    ) {
        let starting_observations = extract_starting_price_observations(source, body);
        if !starting_observations.is_empty() {
            return starting_observations;
        }
    }

    let price_regex =
        Regex::new(r"\$([0-9]{2,3})(?:\.[0-9]{2})?").expect("price regex should compile");
    let speed_regex =
        Regex::new(r"(?i)([0-9](?:[0-9,]*)(?:\.[0-9]+)?)\s*(gig|gbps|gigs|mbps|meg|megs)")
            .expect("speed regex should compile");

    let mut observations = Vec::new();

    for capture in price_regex.captures_iter(body) {
        let Some(price_match) = capture.get(1) else {
            continue;
        };
        let Some(full_match) = capture.get(0) else {
            continue;
        };
        let Ok(price) = price_match.as_str().parse::<f32>() else {
            continue;
        };
        if !(25.0..=180.0).contains(&price) {
            continue;
        }

        let (_window_start, monthly_snippet) =
            snippet_around(body, full_match.start(), full_match.end(), 220, 260);
        let (_local_window_start, local_snippet) =
            snippet_around(body, full_match.start(), full_match.end(), 40, 100);
        if !is_likely_monthly_price(monthly_snippet, local_snippet) {
            continue;
        }

        let downstream_mbps = extract_nearest_speed_mbps(&speed_regex, body, full_match.start());
        let plan_name = pricing_plan_name(source, downstream_mbps, price);

        observations.push(PricingObservation {
            provider_name: source.provider_name.clone(),
            service_type: source.service_type.clone(),
            plan_name,
            downstream_mbps,
            monthly_price_usd: price,
            source_url: source.url.clone(),
        });
    }

    dedupe_pricing_observations(observations)
}

fn extract_starting_price_observations(
    source: &PricingSourceSpec,
    body: &str,
) -> Vec<PricingObservation> {
    let starting_price_regex = Regex::new(
        r"(?is)(?:plans?\s+)?start(?:s|ing)?(?:\s+(?:at|from))?[^$]{0,80}\$([0-9]{2,3})(?:\.[0-9]{2})?\s*(?:/|per\s*)?(?:mo|month)",
    )
    .expect("starting price regex should compile");

    starting_price_regex
        .captures_iter(body)
        .filter_map(|capture| {
            let price = capture.get(1)?.as_str().parse::<f32>().ok()?;
            if !(25.0..=180.0).contains(&price) {
                return None;
            }

            Some(PricingObservation {
                provider_name: source.provider_name.clone(),
                service_type: source.service_type.clone(),
                plan_name: format!(
                    "Observed public {} starting price at ${price:.0}/mo",
                    service_label(&source.service_type)
                ),
                downstream_mbps: None,
                monthly_price_usd: price,
                source_url: source.url.clone(),
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .take(1)
        .collect()
}

fn is_likely_monthly_price(monthly_snippet: &str, local_snippet: &str) -> bool {
    let normalized_monthly_snippet = monthly_snippet.to_ascii_lowercase();
    let normalized_local_snippet = local_snippet.to_ascii_lowercase();
    let has_monthly_context = ["/mo", "per month", "monthly rate", "/month", "mo."]
        .iter()
        .any(|needle| normalized_local_snippet.contains(needle))
        || (normalized_local_snippet.contains("monthly")
            && normalized_monthly_snippet.contains("internet"));
    let has_non_plan_context = [
        "amazon",
        "bill credit",
        "card",
        "connection charge",
        "discount",
        "gift card",
        "off",
        "prepaid",
        "reward card",
        "savings",
        "prepaid card",
        "rebate",
        "value",
        "virtual",
        "back",
        "bonus",
        "save up to",
        "credit",
        "installation",
        "fee",
        "deposit",
        "phone",
        "tablet",
        "watch",
    ]
    .iter()
    .any(|needle| normalized_local_snippet.contains(needle));

    has_monthly_context && !has_non_plan_context
}

fn extract_nearest_speed_mbps(speed_regex: &Regex, body: &str, price_start: usize) -> Option<u32> {
    let (window_start, window) = snippet_around(body, price_start, price_start, 220, 260);

    speed_regex
        .captures_iter(window)
        .filter_map(|capture| {
            let full_match = capture.get(0)?;
            let distance = window_start
                .saturating_add(full_match.start())
                .abs_diff(price_start);
            let value = capture.get(1)?.as_str();
            let unit = capture.get(2)?.as_str().to_ascii_lowercase();
            let mbps = if matches!(unit.as_str(), "gig" | "gbps" | "gigs") {
                decimal_to_scaled_u32(value, 1_000)
            } else {
                decimal_to_scaled_u32(value, 1)
            }?;
            Some((distance, mbps))
        })
        .min_by_key(|(distance, _mbps)| *distance)
        .map(|(_distance, mbps)| mbps)
}

fn snippet_around(
    body: &str,
    start: usize,
    end: usize,
    before: usize,
    after: usize,
) -> (usize, &str) {
    let mut window_start = start.saturating_sub(before);
    let mut window_end = end.saturating_add(after).min(body.len());

    while window_start > 0 && !body.is_char_boundary(window_start) {
        window_start -= 1;
    }
    while window_end < body.len() && !body.is_char_boundary(window_end) {
        window_end += 1;
    }

    (window_start, &body[window_start..window_end])
}

fn decimal_to_scaled_u32(raw_value: &str, scale: u32) -> Option<u32> {
    let value = raw_value.replace(',', "");
    let mut parts = value.split('.');
    let whole = parts.next()?.parse::<u32>().ok()?;
    let fraction = parts.next();
    if parts.next().is_some() {
        return None;
    }

    let scaled_whole = whole.checked_mul(scale)?;
    let scaled_fraction = fraction
        .filter(|digits| !digits.is_empty())
        .and_then(|digits| {
            let denominator = 10_u32.checked_pow(u32::try_from(digits.len()).ok()?)?;
            let numerator = digits.parse::<u32>().ok()?;
            numerator.checked_mul(scale)?.checked_div(denominator)
        })
        .unwrap_or(0);

    scaled_whole.checked_add(scaled_fraction)
}

fn pricing_plan_name(
    source: &PricingSourceSpec,
    downstream_mbps: Option<u32>,
    monthly_price_usd: f32,
) -> String {
    match downstream_mbps {
        Some(speed) if speed > 0 => format!(
            "Observed public {} plan at {}",
            service_label(&source.service_type),
            format_mbps(speed)
        ),
        _ => format!(
            "Observed public {} price at ${monthly_price_usd:.0}/mo",
            service_label(&source.service_type)
        ),
    }
}

fn dedupe_pricing_observations(observations: Vec<PricingObservation>) -> Vec<PricingObservation> {
    let mut deduped = Vec::new();

    for observation in observations {
        let already_seen = deduped.iter().any(|existing: &PricingObservation| {
            existing.provider_name == observation.provider_name
                && existing.downstream_mbps == observation.downstream_mbps
                && (existing.monthly_price_usd - observation.monthly_price_usd).abs() < f32::EPSILON
        });
        if !already_seen {
            deduped.push(observation);
        }
    }

    deduped
}

fn normalized_provider_name(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalize_cache_key(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_display_provider_name(name: &str) -> String {
    match provider_key(name).as_str() {
        "xfinity" => "Xfinity".into(),
        "spectrum" => "Spectrum".into(),
        "att" => "AT&T".into(),
        "tmobile" => "T-Mobile".into(),
        "verizon" => "Verizon".into(),
        "starlink" => "Starlink".into(),
        "hughesnet" => "HughesNet".into(),
        "viasat" => "Viasat".into(),
        "frontier" => "Frontier".into(),
        "cox" => "Cox".into(),
        "optimum" => "Optimum".into(),
        "astound" => "Astound Broadband".into(),
        "brightspeed" => "Brightspeed".into(),
        "centurylink" => "CenturyLink".into(),
        "earthlink" => "EarthLink".into(),
        "googlefiber" => "Google Fiber".into(),
        "kinetic" => "Kinetic".into(),
        "mediacom" => "Mediacom".into(),
        "metronet" => "Metronet".into(),
        "sparklight" => "Sparklight".into(),
        "wow" => "WOW!".into(),
        "ziply" => "Ziply Fiber".into(),
        _ => name.into(),
    }
}

fn provider_key(name: &str) -> String {
    let normalized_name = normalized_provider_name(name);

    for (key, aliases) in [
        ("xfinity", &["xfinity", "comcast"][..]),
        ("spectrum", &["spectrum", "charter"]),
        ("att", &["att", "attfiber", "bellsouth"]),
        ("tmobile", &["tmobile"]),
        ("verizon", &["verizon"]),
        ("starlink", &["starlink", "spacex", "spaceexploration"]),
        ("hughesnet", &["hughesnet", "hughes"]),
        ("viasat", &["viasat"]),
        ("quantum", &["quantum"]),
        ("frontier", &["frontier"]),
        ("cox", &["cox"]),
        ("optimum", &["optimum", "altice"]),
        ("astound", &["astound", "rcn", "wavebroadband", "grande"]),
        ("brightspeed", &["brightspeed"]),
        ("centurylink", &["centurylink", "lumen"]),
        ("earthlink", &["earthlink"]),
        ("googlefiber", &["googlefiber", "google"]),
        ("kinetic", &["kinetic", "windstream"]),
        ("mediacom", &["mediacom"]),
        ("metronet", &["metronet"]),
        ("sparklight", &["sparklight", "cableone"]),
        ("wow", &["wow"]),
        ("ziply", &["ziply"]),
    ] {
        if aliases.iter().any(|alias| normalized_name.contains(alias)) {
            return key.into();
        }
    }

    normalized_name
}

fn provider_names_match(left: &str, right: &str) -> bool {
    provider_key(left) == provider_key(right)
}

fn pricing_sources_for_providers(
    pricing_sources: &[PricingSourceSpec],
    providers: &[InternetProvider],
) -> Vec<PricingSourceSpec> {
    if providers.is_empty() {
        return Vec::new();
    }

    pricing_sources
        .iter()
        .filter(|source| {
            providers
                .iter()
                .any(|provider| provider_names_match(&source.provider_name, &provider.name))
        })
        .cloned()
        .collect()
}

fn service_type_from_technology(technology: &str) -> ServiceType {
    match technology.to_ascii_lowercase().as_str() {
        "fiber" => ServiceType::Fiber,
        "cable" => ServiceType::Cable,
        "dsl" => ServiceType::Dsl,
        "fixed wireless" => ServiceType::FixedWireless,
        "gso satellite" | "leo satellite" => ServiceType::Satellite,
        _ => ServiceType::Unknown,
    }
}

fn service_type_from_technology_code(technology_code: u16) -> ServiceType {
    match technology_code {
        10 => ServiceType::Dsl,
        40 => ServiceType::Cable,
        50 => ServiceType::Fiber,
        60 | 61 => ServiceType::Satellite,
        70..=72 => ServiceType::FixedWireless,
        _ => ServiceType::Unknown,
    }
}

fn technology_label_from_code(technology_code: u16) -> &'static str {
    match technology_code {
        10 => "copper wire",
        40 => "cable",
        50 => "fiber",
        60 => "geostationary satellite",
        61 => "low earth orbit satellite",
        70 => "unlicensed fixed wireless",
        71 => "licensed fixed wireless",
        72 => "licensed-by-rule fixed wireless",
        _ => "internet",
    }
}

fn best_service_type(first: &ServiceType, second: &ServiceType) -> ServiceType {
    if service_type_rank(second) < service_type_rank(first) {
        second.clone()
    } else {
        first.clone()
    }
}

fn service_type_rank(service_type: &ServiceType) -> u8 {
    match service_type {
        ServiceType::Fiber => 0,
        ServiceType::Cable => 1,
        ServiceType::FixedWireless => 2,
        ServiceType::Dsl => 3,
        ServiceType::Satellite => 4,
        ServiceType::Unknown => 5,
    }
}

fn best_availability(first: &Availability, second: &Availability) -> Availability {
    if availability_rank(second) < availability_rank(first) {
        second.clone()
    } else {
        first.clone()
    }
}

fn availability_rank(availability: &Availability) -> u8 {
    match availability {
        Availability::Confirmed => 0,
        Availability::Likely => 1,
        Availability::Unknown => 2,
    }
}

fn service_label(service_type: &ServiceType) -> &'static str {
    match service_type {
        ServiceType::Fiber => "fiber",
        ServiceType::Cable => "cable",
        ServiceType::FixedWireless => "fixed wireless",
        ServiceType::Satellite => "satellite",
        ServiceType::Dsl => "DSL",
        ServiceType::Unknown => "internet",
    }
}

fn badges_for_service_type(technology: &str) -> Vec<String> {
    match technology.to_ascii_lowercase().as_str() {
        "fiber" => vec!["FCC-derived".into(), "fiber".into()],
        "cable" => vec!["FCC-derived".into(), "cable".into()],
        "fixed wireless" => vec!["FCC-derived".into(), "fixed wireless".into()],
        "gso satellite" | "leo satellite" => vec!["FCC-derived".into(), "satellite".into()],
        "dsl" => vec!["FCC-derived".into(), "DSL".into()],
        _ => vec!["FCC-derived".into()],
    }
}

fn pricing_sources_from_env() -> Vec<PricingSourceSpec> {
    let mut sources = default_pricing_sources();

    if let Ok(raw_sources) = env::var(PRICING_SOURCES_ENV) {
        sources.extend(parse_pricing_sources(&raw_sources));
    }

    sources
}

fn default_pricing_sources() -> Vec<PricingSourceSpec> {
    let mut sources = default_cable_pricing_sources();
    sources.extend(default_fiber_pricing_sources());
    sources.extend(default_wireless_pricing_sources());
    sources
}

fn default_cable_pricing_sources() -> Vec<PricingSourceSpec> {
    vec![
        pricing_source(
            "Xfinity",
            ServiceType::Cable,
            "https://www.xfinity.com/learn/internet-service",
        ),
        pricing_source(
            "Spectrum",
            ServiceType::Cable,
            "https://www.spectrum.com/internet",
        ),
        pricing_source(
            "Astound Broadband",
            ServiceType::Cable,
            "https://www.astound.com/internet/",
        ),
        pricing_source(
            "Cox",
            ServiceType::Cable,
            "https://www.cox.com/residential/internet.html",
        ),
        pricing_source(
            "Optimum",
            ServiceType::Cable,
            "https://www.optimum.com/internet",
        ),
        pricing_source(
            "Mediacom",
            ServiceType::Cable,
            "https://mediacomcable.com/products/internet/",
        ),
        pricing_source(
            "Sparklight",
            ServiceType::Cable,
            "https://www.sparklight.com/internet",
        ),
        pricing_source(
            "WOW!",
            ServiceType::Cable,
            "https://www.wowway.com/internet",
        ),
    ]
}

fn default_fiber_pricing_sources() -> Vec<PricingSourceSpec> {
    vec![
        pricing_source(
            "AT&T Fiber",
            ServiceType::Fiber,
            "https://www.att.com/internet/fiber/",
        ),
        pricing_source(
            "Frontier",
            ServiceType::Fiber,
            "https://frontier.com/shop/internet",
        ),
        pricing_source(
            "Google Fiber",
            ServiceType::Fiber,
            "https://fiber.google.com/internet/",
        ),
        pricing_source(
            "Quantum Fiber",
            ServiceType::Fiber,
            "https://www.quantumfiber.com/internet.html",
        ),
        pricing_source(
            "CenturyLink",
            ServiceType::Dsl,
            "https://www.centurylink.com/home/internet/",
        ),
        pricing_source(
            "Brightspeed",
            ServiceType::Fiber,
            "https://www.brightspeed.com/internet/",
        ),
        pricing_source(
            "EarthLink",
            ServiceType::Fiber,
            "https://www.earthlink.net/internet/",
        ),
        pricing_source(
            "Kinetic",
            ServiceType::Fiber,
            "https://www.windstream.com/high-speed-internet",
        ),
        pricing_source(
            "Metronet",
            ServiceType::Fiber,
            "https://www.metronet.com/internet",
        ),
        pricing_source(
            "Ziply Fiber",
            ServiceType::Fiber,
            "https://ziplyfiber.com/internet",
        ),
    ]
}

fn default_wireless_pricing_sources() -> Vec<PricingSourceSpec> {
    vec![
        pricing_source(
            "T-Mobile 5G Home Internet",
            ServiceType::FixedWireless,
            "https://www.t-mobile.com/home-internet",
        ),
        pricing_source(
            "Verizon 5G Home Internet",
            ServiceType::FixedWireless,
            "https://www.verizon.com/home/internet/5g/",
        ),
        pricing_source(
            "Starlink",
            ServiceType::Satellite,
            "https://www.starlink.com/residential",
        ),
    ]
}

fn parse_pricing_sources(raw_sources: &str) -> Vec<PricingSourceSpec> {
    raw_sources
        .split(';')
        .filter_map(|raw_source| {
            let mut parts = raw_source.split('|').map(str::trim);
            let provider_name = parts.next()?;
            let service_type = parts.next()?;
            let url = parts.next()?;

            if provider_name.is_empty() || service_type.is_empty() || url.is_empty() {
                return None;
            }

            Some(pricing_source(
                provider_name,
                parse_service_type(service_type),
                url,
            ))
        })
        .collect()
}

fn pricing_source(provider_name: &str, service_type: ServiceType, url: &str) -> PricingSourceSpec {
    PricingSourceSpec {
        provider_name: provider_name.into(),
        service_type,
        url: url.into(),
    }
}

fn parse_service_type(service_type: &str) -> ServiceType {
    match service_type
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .as_str()
    {
        "fiber" => ServiceType::Fiber,
        "cable" => ServiceType::Cable,
        "fixed wireless" => ServiceType::FixedWireless,
        "satellite" => ServiceType::Satellite,
        "dsl" => ServiceType::Dsl,
        _ => ServiceType::Unknown,
    }
}

fn format_mbps(value: u32) -> String {
    if value >= 1_000 && value.is_multiple_of(1_000) {
        format!("{} Gbps", value / 1_000)
    } else {
        format!("{value} Mbps")
    }
}

fn upstream_error(source: &str, error: &reqwest::Error) -> SearchError {
    SearchError::Upstream {
        source_name: source.into(),
        message: error.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct GeocodedAddress {
    matched_address: String,
    location: GeocodedLocation,
}

#[derive(Debug, Deserialize)]
struct CensusGeocoderResponse {
    result: CensusGeocoderResult,
}

#[derive(Debug, Deserialize)]
struct CensusGeocoderResult {
    #[serde(rename = "addressMatches")]
    address_matches: Vec<CensusAddressMatch>,
}

#[derive(Debug, Deserialize)]
struct CensusAddressMatch {
    coordinates: CensusCoordinates,
    geographies: Option<CensusGeographies>,
    #[serde(rename = "matchedAddress")]
    matched_address: String,
}

impl CensusAddressMatch {
    fn census_block_geoid(&self) -> Option<String> {
        self.geographies
            .as_ref()
            .and_then(|geographies| {
                geographies
                    .census_blocks_2020
                    .as_ref()
                    .or(geographies.census_blocks.as_ref())
            })
            .and_then(|blocks| blocks.first())
            .map(|block| block.geoid.clone())
    }
}

#[derive(Debug, Deserialize)]
struct CensusCoordinates {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct CensusGeographies {
    #[serde(rename = "Census Blocks")]
    census_blocks: Option<Vec<CensusBlock>>,
    #[serde(rename = "2020 Census Blocks")]
    census_blocks_2020: Option<Vec<CensusBlock>>,
}

#[derive(Debug, Deserialize)]
struct CensusBlock {
    #[serde(rename = "GEOID")]
    geoid: String,
}

#[derive(Debug, Deserialize)]
struct BroadbandMapInternetResponse {
    providers: Vec<BroadbandMapProvider>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct BroadbandMapProvider {
    name: String,
    technology: String,
    max_download_mbps: u32,
    max_upload_mbps: u32,
}

#[derive(Debug, Deserialize)]
struct ArcgisQueryResponse {
    features: Vec<ArcgisFeature>,
}

#[derive(Debug, Deserialize)]
struct ArcgisFeature {
    attributes: ArcgisBdcRecord,
}

#[derive(Debug, Deserialize)]
struct ArcgisBdcRecord {
    #[serde(rename = "ProviderName")]
    provider_name: String,
    #[serde(rename = "Technology")]
    technology: u16,
    #[serde(rename = "TotalBSLs")]
    total_bsls: u32,
    #[serde(rename = "UnservedBSLs")]
    unserved_bsls: u32,
    #[serde(rename = "UnderservedBSLs")]
    underserved_bsls: u32,
    #[serde(rename = "ServedBSLs")]
    served_bsls: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_address_whitespace() {
        let query = AddressQuery {
            address: "  123   Main St   ".into(),
        };

        assert_eq!(query.normalized(), "123 Main St");
    }

    #[tokio::test]
    async fn rejects_empty_address_before_live_source_calls() {
        let aggregator = ProviderAggregator::with_pricing_sources(None, vec![]);
        let query = AddressQuery {
            address: "   ".into(),
        };

        assert_eq!(
            aggregator.search(&query).await,
            Err(SearchError::EmptyAddress)
        );
    }

    #[test]
    fn maps_broadband_map_provider_to_unpriced_plan() {
        let provider = map_broadband_map_provider(&BroadbandMapProvider {
            name: "Example Fiber".into(),
            technology: "Fiber".into(),
            max_download_mbps: 2_000,
            max_upload_mbps: 1_000,
        });

        assert_eq!(provider.service_type, ServiceType::Fiber);
        assert_eq!(provider.availability, Availability::Likely);
        assert_eq!(provider.plans[0].downstream_mbps, Some(2_000));
        assert_eq!(provider.plans[0].upstream_mbps, Some(1_000));
        assert_eq!(provider.plans[0].monthly_price_usd, None);
        assert_eq!(provider.plans[0].contract_required, None);
    }

    #[test]
    fn summary_counts_only_real_prices() {
        let provider = InternetProvider {
            name: "Example".into(),
            service_type: ServiceType::Cable,
            availability: Availability::Likely,
            availability_confidence: 90,
            availability_evidence: "Test".into(),
            headline: "Example".into(),
            plans: vec![InternetPlan {
                name: "Reported maximum cable offering".into(),
                downstream_mbps: Some(1_200),
                upstream_mbps: Some(100),
                speed_basis: SpeedBasis::ReportedMaximum,
                monthly_price_usd: None,
                promo_months: None,
                equipment_fee_usd: None,
                install_fee_usd: None,
                data_cap_gb: None,
                contract_required: None,
                notes: vec![],
            }],
            source: ProviderSource {
                label: "Test".into(),
                url: None,
            },
            badges: vec![],
            notes: vec![],
        };

        let pricing_collection = PricingCollection {
            observations: vec![],
            source_count: 1,
            failure_count: 0,
        };
        let summary = summarize(&[provider], &pricing_collection);

        assert_eq!(summary.provider_count, 1);
        assert_eq!(summary.priced_plan_count, 0);
        assert_eq!(summary.cheapest_monthly_price_usd, None);
        assert_eq!(summary.fastest_downstream_mbps, Some(1_200));
        assert_eq!(summary.pricing_source_count, 1);
    }

    #[test]
    fn extracts_public_pricing_observations_from_markup() {
        let source = pricing_source("Example Fiber", ServiceType::Fiber, "https://example.test");
        let body = r"
            <section>
                <h2>Fiber 500</h2>
                <p>500 Mbps internet for $55/mo with autopay.</p>
            </section>
            <section>
                <h2>Fiber 1 Gig</h2>
                <p>1 Gig internet for $75/mo.</p>
            </section>
        ";

        let observations = extract_pricing_observations(&source, body);

        assert_eq!(observations.len(), 2);
        assert!((observations[0].monthly_price_usd - 55.0).abs() < f32::EPSILON);
        assert_eq!(observations[0].downstream_mbps, Some(500));
        assert!((observations[1].monthly_price_usd - 75.0).abs() < f32::EPSILON);
        assert_eq!(observations[1].downstream_mbps, Some(1_000));
    }

    #[test]
    fn ignores_non_monthly_reward_prices() {
        let source = pricing_source("Example Cable", ServiceType::Cable, "https://example.test");
        let body =
            "Get a $200 reward card when you switch. Internet starts at $55/mo for 500 Mbps.";

        let observations = extract_pricing_observations(&source, body);

        assert_eq!(observations.len(), 1);
        assert!((observations[0].monthly_price_usd - 55.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fixed_wireless_uses_starting_price_without_perks() {
        let source = pricing_source(
            "Example 5G Home Internet",
            ServiceType::FixedWireless,
            "https://example.test",
        );
        let body = "Now home internet starting at $35/mo. Get up to $200 back and $100 in streaming perks.";

        let observations = extract_pricing_observations(&source, body);

        assert_eq!(observations.len(), 1);
        assert!((observations[0].monthly_price_usd - 35.0).abs() < f32::EPSILON);
    }

    #[test]
    fn merges_pricing_observations_without_confirming_availability() {
        let mut providers = Vec::new();
        let observations = vec![PricingObservation {
            provider_name: "Example Cable".into(),
            service_type: ServiceType::Cable,
            plan_name: "Observed public cable plan at 300 Mbps".into(),
            downstream_mbps: Some(300),
            monthly_price_usd: 49.0,
            source_url: "https://example.test".into(),
        }];

        merge_pricing_observations(&mut providers, &observations);

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].availability, Availability::Unknown);
        assert_eq!(providers[0].plans[0].monthly_price_usd, Some(49.0));
    }
}
