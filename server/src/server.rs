use axum::response::sse::{Event, Sse};
use axum::{
    extract::{Json, Path as AxumPath, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use fetching_core_lib::{
    coordinator::DownloadCoordinator,
    db::Database,
    spotify_api::{CoverFetcher, SpotifyCollectionMetadata},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::handlers::pwa_handler;

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<DownloadCoordinator>,
    pub db: Arc<Database>,
    /// Shared with the worker — same Arc<Cache> so cover fetches are deduplicated.
    pub cover: Arc<dyn CoverFetcher>,
    /// Used in the POST handler to resolve a URI before queuing.
    pub collection_metadata: Arc<dyn SpotifyCollectionMetadata + Send + Sync>,
}

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct QueueRequest {
    url: String,
}

/// Response for `POST /api/queue`.
#[derive(Serialize)]
pub struct QueueResponse {
    pub collection_id: String,
    pub track_ids: Vec<String>,
    pub task_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn queue_url(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<QueueRequest>,
) -> impl IntoResponse {
    let url = payload.url.clone();

    // Step 1: resolve URI → TrackCollection (blocking librespot call)
    let meta = Arc::clone(&state.collection_metadata);
    let collection = match tokio::task::spawn_blocking(move || meta.fetch_by_uri(&url)).await {
        Ok(Ok(c)) => Arc::new(c),
        Ok(Err(e)) => {
            tracing::warn!("Failed to resolve '{}': {e}", payload.url);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Panic resolving '{}': {e}", payload.url);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal server error" })),
            )
                .into_response();
        }
    };

    // Step 2: enqueue — downloads start here, after the response is ready
    let (collection_id, id_pairs) = state.queue.add_collection(Arc::clone(&collection));

    let track_ids: Vec<String> = id_pairs.iter().map(|(tid, _)| tid.clone()).collect();
    let task_ids: Vec<String> = id_pairs.iter().map(|(_, tid)| tid.clone()).collect();

    tracing::info!("Queued '{}' ({} tracks)", collection.title, task_ids.len());

    (
        StatusCode::OK,
        Json(QueueResponse {
            collection_id,
            track_ids,
            task_ids,
        }),
    )
        .into_response()
}

async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pending = state.queue.len();
    axum::response::Html(format!("<pre>Status: ok — {pending} track(s) pending</pre>"))
}

async fn get_collections(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.db.list_collections() {
        Ok(collections) => (StatusCode::OK, Json(serde_json::to_value(collections).unwrap_or_default())).into_response(),
        Err(e) => {
            tracing::error!("Failed to list collections: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

async fn get_collection_tracks(
    State(state): State<Arc<AppState>>,
    AxumPath(collection_id): AxumPath<String>,
) -> impl IntoResponse {
    match state.db.get_tracks_for_collection(&collection_id) {
        Ok(tracks) => (StatusCode::OK, Json(serde_json::to_value(tracks).unwrap_or_default())).into_response(),
        Err(e) => {
            tracing::error!("Failed to get tracks for collection {collection_id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    }
}

async fn events(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut rx = state.queue.subscribe_progress();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(update) => {
                    if let Ok(json) = serde_json::to_string(&update) {
                        yield Ok::<_, std::convert::Infallible>(Event::default().data(json));
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    };
    Sse::new(stream)
}

pub fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/queue", axum::routing::post(queue_url))
        .route("/api/collections", get(get_collections))
        .route("/api/collections/:id/tracks", get(get_collection_tracks))
        .route("/api/status", get(get_status))
        .route("/events", get(events))
        .fallback(pwa_handler)
        .with_state(state)
}
