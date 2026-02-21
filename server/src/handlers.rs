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
    let res = ServeDir::new("static/pwa")
        .fallback(ServeFile::new("static/pwa/index.html"))
        .oneshot(req)
        .await;
    res.into_response()
}

#[cfg(not(debug_assertions))]
async fn pwa_handler_release(uri: Uri) -> impl IntoResponse {
    use axum::http::StatusCode;
    use crate::assets::PwaAssets;

    let path = uri.path().trim_start_matches('/');
    // Release: Serve embedded asset if it exists
    if let Some(asset) = PwaAssets::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        return ([("content-type", mime.as_str())], asset.data).into_response();
    }

    // SPA fallback for unknown routes (no dot)
    if !path.contains('.') {
        if let Some(index) = PwaAssets::get("index.html") {
            return ([("content-type", "text/html")], index.data).into_response();
        }
    }

    (StatusCode::NOT_FOUND, "Not found").into_response()
}
