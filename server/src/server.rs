use axum::response::sse::{Event, Sse};
use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use fetching_core_lib::{
    container::TrackCollection,
    queue::{CoverFetcher, TaskStatus},
    queue_tokio::TokioQueue,
    spotify_api::SpotifyCollectionMetadata,
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
    pub queue: Arc<TokioQueue>,
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

#[derive(Serialize)]
pub struct QueueResponse {
    pub collection: TrackCollection,
    /// Base64-encoded JPEG as `data:image/jpeg;base64,…`, or `null` if unavailable.
    pub cover_data_url: Option<String>,
    /// Task IDs, in the same order as `collection.track_uris`.
    /// The frontend stores these as `TrackItem.id` values so it can match
    /// incoming SSE `ProgressUpdate` events to individual tracks.
    pub task_ids: Vec<String>,
    /// Current status of each task, in the same order as `task_ids`.
    /// Always populated by `GET /api/queue`; empty in `POST /api/queue`
    /// responses (new tasks always start as `Pending`).
    #[serde(default)]
    pub task_statuses: Vec<TaskStatus>,
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

    // Step 2: fetch cover art inside spawn_blocking — LibrespotCoverFetcher
    // calls futures::executor::block_on internally, which blocks the OS thread.
    // Running it in spawn_blocking keeps the async executor free.
    let cover_data_url = match &collection.cover_id {
        Some(cover_id) => {
            let cover = Arc::clone(&state.cover);
            let cid = cover_id.clone();
            let handle = tokio::runtime::Handle::current();
            match tokio::task::spawn_blocking(move || {
                handle.block_on(cover.fetch_cover(&cid))
            })
            .await
            {
                Ok(Ok(bytes)) => Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(&bytes))),
                Ok(Err(e)) => {
                    tracing::warn!("Cover fetch failed for '{}': {e}", collection.title);
                    None
                }
                Err(e) => {
                    tracing::warn!("Cover fetch panicked for '{}': {e}", collection.title);
                    None
                }
            }
        }
        None => None,
    };

    // Step 3: enqueue — downloads start here, after the response is ready
    let task_ids: Vec<String> = state
        .queue
        .add_collection(Arc::clone(&collection))
        .into_iter()
        .map(|id| id.to_string())
        .collect();

    tracing::info!("Queued '{}' ({} tracks)", collection.title, task_ids.len());

    (
        StatusCode::OK,
        Json(QueueResponse {
            collection: (*collection).clone(),
            cover_data_url,
            task_ids,
            task_statuses: vec![],
        }),
    )
        .into_response()
}

async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pending = state.queue.len();
    axum::response::Html(format!("<pre>Status: ok — {pending} track(s) pending</pre>"))
}

async fn get_queue(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snapshots = match state.queue.snapshot() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to read queue snapshot: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // Group tasks by collection, preserving track_uris order.
    // Use IndexMap so response order is stable (insertion = enqueue order).
    let mut by_collection: std::collections::HashMap<
        String,
        (std::sync::Arc<fetching_core_lib::container::TrackCollection>, Vec<(usize, String, TaskStatus)>),
    > = std::collections::HashMap::new();

    for snap in snapshots {
        let entry = by_collection
            .entry(snap.collection.uri_str.clone())
            .or_insert_with(|| (std::sync::Arc::clone(&snap.collection), vec![]));
        let pos = entry
            .0
            .track_uris
            .iter()
            .position(|u| u == &snap.track_uri)
            .unwrap_or(usize::MAX);
        entry.1.push((pos, snap.task_id.to_string(), snap.status));
    }

    let mut responses: Vec<QueueResponse> = Vec::new();
    for (collection, mut tasks) in by_collection.into_values() {
        tasks.sort_by_key(|(pos, _, _)| *pos);

        let cover_data_url = match &collection.cover_id {
            Some(cover_id) => {
                let cover = std::sync::Arc::clone(&state.cover);
                let cid = cover_id.clone();
                let handle = tokio::runtime::Handle::current();
                match tokio::task::spawn_blocking(move || handle.block_on(cover.fetch_cover(&cid))).await {
                    Ok(Ok(bytes)) => Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(&bytes))),
                    _ => None,
                }
            }
            None => None,
        };

        let (task_ids, task_statuses): (Vec<_>, Vec<_>) = tasks
            .into_iter()
            .map(|(_, id, status)| (id, status))
            .unzip();

        responses.push(QueueResponse {
            collection: (*collection).clone(),
            cover_data_url,
            task_ids,
            task_statuses,
        });
    }

    (StatusCode::OK, Json(responses)).into_response()
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
        .route("/", get(pwa_handler))
        .route("/*path", get(pwa_handler))
        .route("/api/queue", axum::routing::post(queue_url))
        .route("/api/queue", get(get_queue))
        .route("/api/status", get(get_status))
        .route("/events", get(events))
        .with_state(state)
}
