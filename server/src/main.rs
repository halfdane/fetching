
use axum::{Router, routing::get, response::IntoResponse};


pub fn app() -> Router {
    Router::new().route("/", get(root))
}

#[tokio::main]
async fn main() {
    let app = app();
    println!("Server running on http://127.0.0.1:8080");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> impl IntoResponse {
    "OK"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::util::ServiceExt; // for `oneshot`
    use hyper::http::Request;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn get_root_returns_ok() {
        let app = app();
        let response = app
            .oneshot(Request::builder().uri("/").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(body, "OK");
    }
}
