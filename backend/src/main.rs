use crate::error::YafdError;
use crate::handler::get_config;
use dotenvy::dotenv;
use handler::post_duplicates_handler;
use models::{AppState, Config};
use std::env;
use tower_http::cors::CorsLayer;
mod business_logic;
mod error;
mod handler;
mod models;

#[tokio::main]
async fn main() -> Result<(), YafdError> {
    dotenv().ok();

    let root_folder = env::var("ROOT_FOLDER").expect("ROOT_FOLDER must be set");
    let target_folder = env::var("TARGET_FOLDER").expect("TARGET_FOLDER must be set");
    let tmp_skip_folders = env::var("SKIP_FOLDERS").expect("SKIP_FOLDERS must be set");
    let tmp_skip_filenames = env::var("SKIP_FILE_NAMES").expect("SKIP_FILE_NAMES must be set");
    let min_file_size = env::var("MIN_FILE_SIZE")
        .expect("MIN_FILE_SIZE must be set")
        .parse::<u64>()
        .expect("should be a number");

    let tmp_consider_extensions =
        env::var("CONSIDER_EXTENSIONS").expect("CONSIDER_EXTENSIONS must be set");

    let bla = &[' ', ',', ';'][..];

    let skip_folders: Vec<String> = tmp_skip_folders
        .split(bla)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let skip_filenames: Vec<String> = tmp_skip_filenames
        .split(bla)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let consider_extensions: Vec<String> = tmp_consider_extensions
        .split(bla)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let default_config = Config {
        root_folder,
        target_folder,
        skip_folders,
        skip_filenames,
        min_file_size,
        consider_extensions,
    };

    println!("default_config {:?}", default_config);

    let app_state = AppState { default_config };

    let app = axum::Router::new()
        .route(
            "/api/duplicates",
            axum::routing::post(post_duplicates_handler),
        )
        .route("/api/config", axum::routing::get(get_config))
        .layer(cors_layer())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:4023").await.unwrap();
    axum::serve(listener, app).await.unwrap();
    Ok(())
}

pub fn cors_layer() -> CorsLayer {
    CorsLayer::permissive()
}
