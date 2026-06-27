use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

/// Md5 digest as a lowercase hex string.
pub type Md5 = String;

/// A single duplicate file copy. Display-only metadata; keep is per-FOLDER,
/// not per-file, so there's no keep flag here.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
}

/// A folder that contains at least one duplicate file copy. The user marks
/// whole folders as "keep in place" (keep=true); unchecked folders' files
/// get moved to target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Folder {
    pub folder: String,
    pub keep: bool,
    pub files: Vec<FileEntry>,
    /// total bytes of duplicate files in this folder
    pub total_size: u64,
}

#[derive(Debug)]
pub struct AppState {
    pub root: PathBuf,
    pub target: PathBuf,
    pub folders: Vec<Folder>,
    /// true while the background scan thread is still running.
    pub scanning: bool,
    /// set if the scan thread panicked / errored.
    pub scan_error: Option<String>,
    /// live counters (also written by the scan thread; read by /api/scan_progress)
    pub walked: Arc<AtomicUsize>,
    pub hashed: Arc<AtomicUsize>,
}

impl AppState {
    pub fn new_for_scan(root: PathBuf, target: PathBuf, scanning: bool) -> Self {
        Self {
            root,
            target,
            folders: Vec::new(),
            scanning,
            scan_error: None,
            walked: Arc::new(AtomicUsize::new(0)),
            hashed: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn walked(&self) -> usize {
        self.walked.load(Ordering::Relaxed)
    }
    pub fn hashed(&self) -> usize {
        self.hashed.load(Ordering::Relaxed)
    }
}

pub type SharedState = Arc<Mutex<AppState>>;
