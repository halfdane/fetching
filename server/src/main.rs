
use axum::{Router, routing::get, response::IntoResponse, extract::{State, Json}};
use serde::Deserialize;
use tower_http::services::ServeDir;
use std::sync::Arc;
use tokio::sync::{mpsc, broadcast};
use uuid::Uuid;



#[derive(Debug, Clone)]
struct Task {
    pub task_id: Uuid,
    pub url: String,
}

#[derive(Clone)]
struct AppState {
    task_tx: mpsc::Sender<Task>,
    progress_tx: broadcast::Sender<String>, // Placeholder type for now
    auth_token: String,
}

pub fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .nest_service("/", ServeDir::new("server/static").not_found_service(axum::routing::get(|| async { axum::http::StatusCode::NOT_FOUND })))
        .route("/api/queue", axum::routing::post(queue_url))
        .route("/api/status", get(get_status))
        .route("/events", get(events))
        .with_state(state)
}

#[tokio::main]
async fn main() {
    // Channel setup (no logic yet)
    let (task_tx, _task_rx) = mpsc::channel::<Task>(100);
    let (progress_tx, _progress_rx) = broadcast::channel::<String>(100);
    let auth_token = "devtoken".to_string();
    let state = Arc::new(AppState {
        task_tx,
        progress_tx,
        auth_token,
    });
    let app = app(state);
    println!("Server running on http://127.0.0.1:8080");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}



// No longer needed: static files are served by ServeDir

#[derive(Deserialize)]
struct QueueRequest {
    url: String,
}

async fn queue_url(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<QueueRequest>,
) -> impl IntoResponse {
    let task_id = Uuid::new_v4();
    let task = Task {
        task_id,
        url: payload.url,
    };
    // Send to worker channel (ignore if full for now)
    let _ = state.task_tx.send(task).await;
    axum::http::StatusCode::ACCEPTED
}

async fn get_status(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: return JSON status
    axum::response::Html("<pre>Status: TODO</pre>")
}

async fn events(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: implement SSE
    axum::response::Html("<pre>SSE: TODO</pre>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::util::ServiceExt; // for `oneshot`
    use hyper::http::{Request, StatusCode};
    use axum::body::to_bytes;

    use std::sync::Arc as StdArc;
    use tokio::sync::{mpsc, broadcast};

    struct TestApp {
        app: axum::Router,
        task_rx: mpsc::Receiver<Task>,
    }

         // Removed test_queue from AppState
    fn test_app_with_queue() -> TestApp {
        let (task_tx, task_rx) = mpsc::channel(8);
        let (progress_tx, _progress_rx) = broadcast::channel(8);
        let state = StdArc::new(AppState {
            task_tx,
            progress_tx,
            auth_token: "devtoken".to_string(),
        });
        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { "OK" }))
            .route("/api/queue", axum::routing::post(queue_url))
            .route("/api/status", axum::routing::get(get_status))
            .route("/events", axum::routing::get(events))
            .with_state(state);
        TestApp { app, task_rx }
    }

    fn test_app() -> axum::Router {
        let (task_tx, _task_rx) = mpsc::channel(8);
        let (progress_tx, _progress_rx) = broadcast::channel(8);
        let state = StdArc::new(AppState {
            task_tx,
            progress_tx,
            auth_token: "devtoken".to_string(),
        });
        axum::Router::new()
            .route("/", axum::routing::get(|| async { "OK" }))
            .route("/api/queue", axum::routing::post(queue_url))
            .route("/api/status", axum::routing::get(get_status))
            .route("/events", axum::routing::get(events))
            .with_state(state)
    }

    #[tokio::test]
    async fn get_root_returns_ok() {
        let app = test_app();
        let response = app
            .oneshot(Request::builder().uri("/").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_queue_returns_accepted_and_queues_task() {
        let TestApp { app, mut task_rx } = test_app_with_queue();
        let body = serde_json::json!({"url": "spotify:track:123"}).to_string();
        let response = app
            .oneshot(Request::builder()
                .method("POST")
                .uri("/api/queue")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        let body_str = std::str::from_utf8(&bytes).unwrap_or("<non-utf8>");
        if status != StatusCode::ACCEPTED {
            panic!("POST /api/queue failed: status = {:?}, body = {}", status, body_str);
        }
        match tokio::time::timeout(std::time::Duration::from_secs(1), task_rx.recv()).await {
            Ok(Some(task)) => {
                assert_eq!(task.url, "spotify:track:123");
            }
            Ok(None) => panic!("[test] Channel closed unexpectedly"),
            Err(_) => panic!("[test] Timed out waiting for task"),
        }
    }

    #[tokio::test]
    async fn get_status_returns_html() {
        let app = test_app();
        let response = app
            .oneshot(Request::builder().uri("/api/status").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(body_str.contains("Status: TODO"));
    }

    #[tokio::test]
    async fn get_events_returns_html() {
        let app = test_app();
        let response = app
            .oneshot(Request::builder().uri("/events").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(body_str.contains("SSE: TODO"));
    }
}
