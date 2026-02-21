use axum::response::sse::{Event, Sse};
use axum::{
    extract::{Json, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use fetching_core::SharedQueue;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<SharedQueue>,
}

#[derive(Deserialize)]
struct QueueRequest {
    url: String,
}

async fn queue_url(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<QueueRequest>,
) -> impl axum::response::IntoResponse {
    let task_id = Uuid::new_v4();
    tracing::info!("Web: queued task {}: {}", task_id, payload.url);

    state.queue.add_tasks(vec![payload.url.clone()]).await;

    axum::http::StatusCode::ACCEPTED
}
async fn get_status(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: return JSON status
    axum::response::Html("<pre>Status: TODO</pre>")
}

async fn events(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut rx = state.queue.as_ref().progress_tx().subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(update) => {
                    if update.status == "end" {
                        break;
                    }
                    if let Ok(json) = serde_json::to_string(&update) {
                        yield Ok::<_, std::convert::Infallible>(Event::default().data(json));
                    }
                },
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    };
    Sse::new(stream)
}

pub fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .nest_service(
            "/",
            ServeDir::new("frontend/build").not_found_service(axum::routing::get(|| async {
                axum::http::StatusCode::NOT_FOUND
            })),
        )
        .route("/api/queue", axum::routing::post(queue_url))
        .route("/api/status", get(get_status))
        .route("/events", get(events))
        .with_state(state)
}

// Remove all test code below
