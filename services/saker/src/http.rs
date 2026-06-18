use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CACHE_CONTROL},
    routing::{get, post},
};
use saker_core::{
    AddressQuery, AddressSuggestion, ProviderAggregator, ProviderSearchResult, SearchError,
};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Debug, Clone, Default)]
pub struct AppState {
    aggregator: ProviderAggregator,
}

pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/search/address/suggest", get(suggest_address))
        .route("/v1/search/address", post(search_address))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(AppState::default())
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn suggest_address(
    State(state): State<AppState>,
    Query(query): Query<SuggestQuery>,
) -> Result<(HeaderMap, Json<Vec<AddressSuggestion>>), ApiError> {
    let suggestions = state
        .aggregator
        .suggest_addresses(&AddressQuery { address: query.q })
        .await
        .map_err(ApiError::from)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=60, stale-while-revalidate=300"),
    );

    Ok((headers, Json(suggestions)))
}

async fn search_address(
    State(state): State<AppState>,
    Json(query): Json<AddressQuery>,
) -> Result<Json<ProviderSearchResult>, ApiError> {
    let result = state
        .aggregator
        .search(&query)
        .await
        .map_err(ApiError::from)?;

    if !result.caveats.is_empty() {
        tracing::info!(address = %result.address, caveats = ?result.caveats, "search caveats");
    }

    Ok(Json(result))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
}

#[derive(Debug, Deserialize)]
struct SuggestQuery {
    q: String,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    NotFound(String),
    Unavailable(String),
    BadGateway(String),
}

impl From<SearchError> for ApiError {
    fn from(value: SearchError) -> Self {
        match value {
            SearchError::EmptyAddress => Self::BadRequest(value.to_string()),
            SearchError::AddressNotFound => Self::NotFound(value.to_string()),
            SearchError::ProviderDataSourceMissing => Self::Unavailable(value.to_string()),
            SearchError::Upstream { .. } => Self::BadGateway(value.to_string()),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::BadRequest(error) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error })),
            Self::NotFound(error) => (StatusCode::NOT_FOUND, Json(ErrorResponse { error })),
            Self::Unavailable(error) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse { error }),
            ),
            Self::BadGateway(error) => (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error })),
        }
        .into_response()
    }
}
