
use axum::{Router, routing::get, response::IntoResponse, extract::{State, Json}};
use axum::response::sse::{self, Sse, Event};
use futures_util::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tokio::sync::{mpsc, broadcast};
use std::sync::Arc;
use spotify_player::{ProgressUpdate, ProgressScope, process_url};
use tower_http::services::ServeDir;


#[derive(Debug, Clone)]
struct Task {
    pub task_id: Uuid,
    pub url: String,
}

#[derive(Clone)]
pub(crate) struct AppState {
    task_tx: mpsc::Sender<Task>,
    progress_tx: broadcast::Sender<ProgressUpdate>,
    auth_token: String,
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
        url: payload.url.clone(),
    };
    // Send to worker channel (ignore if full for now)
    let _ = state.task_tx.send(task).await;
    // Send queued update
    let queued_update = ProgressUpdate {
        task_id,
        scope: ProgressScope::Global,
        status: "queued".to_string(),
        current: 0,
        total: 0,
        item: "".to_string(),
        url: Some(payload.url),
    };
    let _ = state.progress_tx.send(queued_update);
    axum::http::StatusCode::ACCEPTED
}

async fn get_status(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    // TODO: return JSON status
    axum::response::Html("<pre>Status: TODO</pre>")
}

async fn events(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut rx = state.progress_tx.subscribe();
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
        .nest_service("/", ServeDir::new("server/static").not_found_service(axum::routing::get(|| async { axum::http::StatusCode::NOT_FOUND })))
        .route("/api/queue", axum::routing::post(queue_url))
        .route("/api/status", get(get_status))
        .route("/events", get(events))
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();


    // Channel setup (no logic yet)
    let (task_tx, _task_rx) = mpsc::channel::<Task>(100);
    let (progress_tx, _progress_rx) = broadcast::channel::<ProgressUpdate>(100);
    let auth_token = "devtoken".to_string();
    let state = Arc::new(AppState {
        task_tx,
        progress_tx: progress_tx.clone(),
        auth_token,
    });
    // Spawn worker
    tokio::spawn(async move {
        let mut task_rx = _task_rx;
        while let Some(task) = task_rx.recv().await {
            let (tx, mut rx) = mpsc::channel(100);
            let progress_tx_clone = progress_tx.clone();
            let forward_task = tokio::spawn(async move {
                while let Some(update) = rx.recv().await {
                    let _ = progress_tx_clone.send(update);
                }
            });
            if let Err(_e) = process_url(task.task_id, task.url.clone(), tx).await {
                let error_update = ProgressUpdate {
                    task_id: task.task_id,
                    scope: ProgressScope::Global,
                    status: "error".to_string(),
                    current: 0,
                    total: 0,
                    item: "".to_string(),
                    url: Some(task.url),
                };
                let _ = progress_tx.send(error_update);
            }
            let _ = forward_task.await;
        }
    });
    let app = app(state);
    println!("Server running on http://127.0.0.1:8080");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
        use axum::response::sse::{Event, Sse};
        use futures_util::StreamExt;
        use serde_json;
        #[tokio::test]
        async fn post_queue_triggers_progress_sse() {
            // Setup app and channels
            let (task_tx, mut task_rx) = mpsc::channel(8);
            let (progress_tx, _progress_rx) = broadcast::channel(8);
            // Only clone task_tx for AppState after drop
            let app_state_task_tx = task_tx.clone();
            let state = StdArc::new(AppState {
                task_tx: app_state_task_tx,
                progress_tx: progress_tx.clone(),
                auth_token: "devtoken".to_string(),
            });
            // Spawn worker loop (no shutdown channel needed)
            let worker = tokio::spawn({
                let progress_tx = progress_tx.clone();
                async move {
                    while let Some(task) = task_rx.recv().await {
                        println!("[worker] got task: {}", task.url);
                        let started = ProgressUpdate {
                            task_id: task.task_id,
                            scope: ProgressScope::Global,
                            status: "started".to_string(),
                            current: 0,
                            total: 0,
                            item: "".to_string(),
                            url: Some(task.url.clone()),
                        };
                        let _ = progress_tx.send(started);
                        for pct in [25, 50, 75, 100] {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                            let update = ProgressUpdate {
                                task_id: task.task_id,
                                scope: ProgressScope::Global,
                                status: "progress".to_string(),
                                current: pct,
                                total: 100,
                                item: "".to_string(),
                                url: Some(task.url.clone()),
                            };
                            let _ = progress_tx.send(update);
                        }
                        let finished = ProgressUpdate {
                            task_id: task.task_id,
                            scope: ProgressScope::Global,
                            status: "finished".to_string(),
                            current: 100,
                            total: 100,
                            item: "".to_string(),
                            url: Some(task.url.clone()),
                        };
                        let _ = progress_tx.send(finished);
                        let end = ProgressUpdate {
                            task_id: task.task_id,
                            scope: ProgressScope::Global,
                            status: "end".to_string(),
                            current: 0,
                            total: 0,
                            item: "".to_string(),
                            url: Some(task.url.clone()),
                        };
                        let _ = progress_tx.send(end);
                    }
                    println!("[worker] task_rx closed, worker exiting");
                }
            });
            let app = axum::Router::new()
                .route("/api/queue", axum::routing::post(queue_url))
                .route("/events", axum::routing::get(events))
                .with_state(state.clone());

            // Queue a task
            let body = serde_json::json!({"url": "spotify:track:abc"}).to_string();
            let post_response = app.clone().oneshot(Request::builder()
                .method("POST")
                .uri("/api/queue")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body)).unwrap())
                .await
                .unwrap();
            assert_eq!(post_response.status(), StatusCode::ACCEPTED);

            // Drop task_tx after queuing the task
            drop(task_tx);
            println!("[test] dropped all task_tx clones");

            let sse_response = app.clone().oneshot(Request::builder()
                .uri("/events")
                .body(axum::body::Body::empty()).unwrap())
                .await
                .unwrap();

            // Fully poll the SSE response body stream to completion
            use axum::body::Body;
            let body = sse_response.into_body();
            let mut stream = body.into_data_stream();
            let mut buf = Vec::new();
            let mut found_started = false;
            let mut found_finished = false;
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result.expect("SSE body chunk error");
                buf.extend_from_slice(&chunk);
                let text = String::from_utf8_lossy(&buf);
                for line in text.lines() {
                    if let Some(json) = line.strip_prefix("data:") {
                        if let Ok(update) = serde_json::from_str::<ProgressUpdate>(json.trim()) {
                            if update.status == "started" { found_started = true; }
                            if update.status == "finished" { found_finished = true; }
                            if update.status == "end" { break; }
                        }
                    }
                }
            }
            // Explicitly drop the stream to ensure the SSE handler task is cleaned up
            drop(stream);
            assert!(found_started, "Did not receive started event");
            assert!(found_finished, "Did not receive finished event");

            // Explicitly drop responses before dropping app/state
            drop(post_response);

            drop(app);
            drop(state);

            // Wait for worker to finish (optionally with timeout)
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), worker).await;
            println!("[test] worker join complete");
        }
    use super::*;
    use tower::util::ServiceExt; // for `oneshot`
    use hyper::http::{Request, StatusCode};
    use axum::body::to_bytes;
    use uuid::Uuid;

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

}
