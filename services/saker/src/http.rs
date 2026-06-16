use axum::{Json, Router, extract::State, http::StatusCode, routing::get, routing::post};
use saker_core::{AddressQuery, ProviderAggregator, ProviderSearchResult, SearchError};
use serde::Serialize;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Debug, Clone)]
pub struct AppState {
    aggregator: ProviderAggregator,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            aggregator: ProviderAggregator,
        }
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/search/address", post(search_address))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(AppState::default())
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

async fn search_address(
    State(state): State<AppState>,
    Json(query): Json<AddressQuery>,
) -> Result<Json<ProviderSearchResult>, ApiError> {
    state
        .aggregator
        .search(&query)
        .map(Json)
        .map_err(ApiError::from)
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    ok: bool,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
}

impl From<SearchError> for ApiError {
    fn from(value: SearchError) -> Self {
        match value {
            SearchError::EmptyAddress => Self::BadRequest(value.to_string()),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::BadRequest(error) => (StatusCode::BAD_REQUEST, Json(ErrorResponse { error })),
        }
        .into_response()
    }
}
