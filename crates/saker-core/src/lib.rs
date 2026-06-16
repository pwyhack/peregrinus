use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    pub providers: Vec<InternetProvider>,
    pub caveats: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct InternetProvider {
    pub name: String,
    pub service_type: ServiceType,
    pub availability: Availability,
    pub plans: Vec<InternetPlan>,
    pub source: ProviderSource,
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

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct InternetPlan {
    pub name: String,
    pub downstream_mbps: u32,
    pub upstream_mbps: u32,
    pub monthly_price_usd: Option<f32>,
    pub contract_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ProviderSource {
    pub label: String,
    pub url: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SearchError {
    #[error("address must not be empty")]
    EmptyAddress,
}

#[derive(Debug, Default, Clone)]
pub struct ProviderAggregator;

impl ProviderAggregator {
    /// Temporary deterministic catalog until real provider integrations are added.
    ///
    /// Keeping this behind the aggregator boundary gives future agents a focused place to replace
    /// mocked data with FCC, provider API, scraping, or partner-feed integrations.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::EmptyAddress`] when the normalized address has no content.
    pub fn search(&self, query: &AddressQuery) -> Result<ProviderSearchResult, SearchError> {
        let address = query.normalized();
        if address.is_empty() {
            return Err(SearchError::EmptyAddress);
        }

        Ok(ProviderSearchResult {
            address,
            providers: sample_providers(),
            caveats: vec![
                "Prototype data only: availability must be verified with provider sources.".into(),
            ],
        })
    }
}

fn sample_providers() -> Vec<InternetProvider> {
    vec![
        InternetProvider {
            name: "Saker Fiber".into(),
            service_type: ServiceType::Fiber,
            availability: Availability::Likely,
            plans: vec![
                InternetPlan {
                    name: "500 Mbps Fiber".into(),
                    downstream_mbps: 500,
                    upstream_mbps: 500,
                    monthly_price_usd: Some(55.0),
                    contract_required: false,
                },
                InternetPlan {
                    name: "1 Gig Fiber".into(),
                    downstream_mbps: 1_000,
                    upstream_mbps: 1_000,
                    monthly_price_usd: Some(75.0),
                    contract_required: false,
                },
            ],
            source: ProviderSource {
                label: "Seed catalog".into(),
                url: None,
            },
        },
        InternetProvider {
            name: "Mesa Cable".into(),
            service_type: ServiceType::Cable,
            availability: Availability::Unknown,
            plans: vec![InternetPlan {
                name: "Gig Cable".into(),
                downstream_mbps: 940,
                upstream_mbps: 40,
                monthly_price_usd: Some(80.0),
                contract_required: false,
            }],
            source: ProviderSource {
                label: "Seed catalog".into(),
                url: None,
            },
        },
    ]
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

    #[test]
    fn rejects_empty_address() {
        let aggregator = ProviderAggregator;
        let query = AddressQuery {
            address: "   ".into(),
        };

        assert_eq!(aggregator.search(&query), Err(SearchError::EmptyAddress));
    }

    #[test]
    fn returns_seed_providers() {
        let aggregator = ProviderAggregator;
        let query = AddressQuery {
            address: "123 Main St".into(),
        };

        let result = aggregator.search(&query).expect("seed result");

        assert_eq!(result.address, "123 Main St");
        assert!(!result.providers.is_empty());
    }
}
