use axum::debug_handler;
use axum::extract::{Json, State};
use chrono::Utc;

use crate::business_logic::find_duplicates;
use crate::error::YafdError;
use crate::models::{AppState, Config, DuplicateFiles};

#[debug_handler]
pub async fn get_config(State(app_state): State<AppState>) -> Result<Json<Config>, YafdError> {
    println!(
        "returning default config:    {:?}",
        app_state.default_config
    );

    Ok(Json(app_state.default_config))
}

#[debug_handler]
pub async fn post_duplicates_handler(
    State(app_state): State<AppState>,
    Json(config): Json<Config>,
) -> Result<Json<Vec<DuplicateFiles>>, YafdError> {
    let now = Utc::now();

    println!("config from fe:                   {:?}", config);
    println!("app_state with default config:    {:?}", app_state);

    let duplicates = find_duplicates(&config)?;
    let duration = Utc::now() - now;
    println!("finding duplicates took {} secs", duration.num_seconds());

    Ok(Json(duplicates))
}
