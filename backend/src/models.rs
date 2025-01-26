use std::{path::PathBuf, time::SystemTime};

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AFile {
    pub file_name: String,
    pub file_size: u64,
    pub created: SystemTime,
    pub chrono_created: NaiveDateTime,
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DuplicateFiles {
    pub hash: String,
    pub paths: Vec<AFile>,
    pub cnt_duplicates: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub root_folder: String,
    pub target_folder: String,
    pub skip_folders: Vec<String>,
    pub skip_filenames: Vec<String>,
    pub min_file_size: u64,
    pub consider_extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub default_config: Config,
}
