use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use mime_guess::from_path;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "src/static/"]
struct StaticAssets;

fn serve_asset(path: &str) -> Option<Response> {
    let file = StaticAssets::get(path)?;
    let mime = from_path(path).first_or_octet_stream();
    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(axum::body::Body::from(file.data))
            .ok()?,
    )
}

pub async fn index_handler() -> impl IntoResponse {
    serve_asset("index.html").unwrap_or_else(|| {
        (StatusCode::NOT_FOUND, "index.html not found").into_response()
    })
}

/// Serve a vendored static asset: GET /vendor/bootstrap.min.css etc.
/// Files live under src/static/vendor/ and are embedded by rust-embed.
pub async fn vendor_handler(
    axum::extract::Path(file): axum::extract::Path<String>,
) -> impl IntoResponse {
    let asset_path = format!("vendor/{}", file);
    serve_asset(&asset_path).unwrap_or_else(|| {
        (StatusCode::NOT_FOUND, format!("vendor asset not found: {}", file)).into_response()
    })
}
