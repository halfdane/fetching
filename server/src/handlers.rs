use axum::{
    http::Uri,
    response::IntoResponse,
};

pub async fn pwa_handler(uri: Uri) -> impl IntoResponse {
    #[cfg(debug_assertions)]
    {
        return pwa_handler_debug(uri).await;
    }
    #[cfg(not(debug_assertions))]
    {
        return pwa_handler_release(uri).await;
    }
}

#[cfg(debug_assertions)]
async fn pwa_handler_debug(uri: Uri) -> impl IntoResponse {
    use tower_http::services::{ServeDir, ServeFile};
    use axum::{http::Request, body::Body};
    use tower::ServiceExt;
    let req = Request::builder().uri(uri.clone()).body(Body::empty()).unwrap();
    let res = ServeDir::new("frontend/build")
        .fallback(ServeFile::new("frontend/build/index.html"))
        .oneshot(req)
        .await;
    res.into_response()
}

#[cfg(not(debug_assertions))]
async fn pwa_handler_release(uri: Uri) -> impl IntoResponse {
    use axum::http::StatusCode;
    use crate::assets::PwaAssets;

    let path = uri.path().trim_start_matches('/');

    // Determine cache policy based on path:
    //   - sw.js / manifest.json: never cache — browsers use these to detect updates
    //   - _app/immutable/*:      cache forever — Vite content-hashes these filenames
    //   - everything else:       revalidate — allows conditional 304 but no stale serving
    let cache_control = if path == "sw.js" || path == "manifest.json" {
        "no-store"
    } else if path.starts_with("_app/immutable/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    // Release: Serve embedded asset if it exists
    if let Some(asset) = PwaAssets::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        return (
            [
                ("content-type", mime.as_str()),
                ("cache-control", cache_control),
            ],
            asset.data,
        )
            .into_response();
    }

    // SPA fallback for unknown routes (no dot)
    if !path.contains('.') {
        if let Some(index) = PwaAssets::get("index.html") {
            return (
                [
                    ("content-type", "text/html"),
                    ("cache-control", "no-cache"),
                ],
                index.data,
            )
                .into_response();
        }
    }

    (StatusCode::NOT_FOUND, "Not found").into_response()
}
