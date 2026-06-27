use crate::{
    assets::{index_handler, vendor_handler},
    model::{Folder, SharedState},
    move_files::{move_non_kept, plan_move, MovePlanItem},
};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Serialize)]
struct StateResponse {
    root: String,
    target: String,
    folders: Vec<Folder>,
    kept_folders: usize,
    total_files: usize,
    total_size: u64,
    scanning: bool,
    scan_error: Option<String>,
}

async fn get_state(State(st): State<SharedState>) -> Json<StateResponse> {
    let s = st.lock().unwrap();
    let kept_folders = s.folders.iter().filter(|f| f.keep).count();
    let total_files: usize = s.folders.iter().map(|f| f.files.len()).sum();
    let total_size: u64 = s.folders.iter().map(|f| f.total_size).sum();
    Json(StateResponse {
        root: s.root.to_string_lossy().to_string(),
        target: s.target.to_string_lossy().to_string(),
        folders: s.folders.clone(),
        kept_folders,
        total_files,
        total_size,
        scanning: s.scanning,
        scan_error: s.scan_error.clone(),
    })
}

#[derive(Serialize)]
struct ScanProgress {
    scanning: bool,
    scan_error: Option<String>,
    walked: usize,
    hashed: usize,
}

/// Live scan progress for the web UI to poll.
async fn scan_progress(State(st): State<SharedState>) -> Json<ScanProgress> {
    let s = st.lock().unwrap();
    Json(ScanProgress {
        scanning: s.scanning,
        scan_error: s.scan_error.clone(),
        walked: s.walked(),
        hashed: s.hashed(),
    })
}

#[derive(Deserialize)]
struct MarkFolderBody {
    folder: String,
    keep: bool,
}

/// Mark a folder as keep (or not). Cascades to subfolders: any folder whose
/// path starts with the given folder is set to the same keep value, so a user
/// can "keep everything under /photos/2024" with one click.
async fn mark_folder(
    State(st): State<SharedState>,
    Json(body): Json<MarkFolderBody>,
) -> Json<serde_json::Value> {
    let mut s = st.lock().unwrap();
    let target = fs::canonicalize(&body.folder).unwrap_or_else(|_| PathBuf::from(&body.folder));
    let mut count = 0;
    for f in s.folders.iter_mut() {
        let f_canon = fs::canonicalize(&f.folder).unwrap_or_else(|_| PathBuf::from(&f.folder));
        // recursive cascade: this folder OR any subfolder of it
        if f_canon == target || f_canon.starts_with(&target) {
            f.keep = body.keep;
            count += 1;
        }
    }
    Json(serde_json::json!({"ok": count > 0, "marked": count}))
}

#[derive(Deserialize)]
struct MarkAllBody {
    keep: bool,
}

/// Mark every folder as keep (or un-keep every folder).
async fn mark_all(
    State(st): State<SharedState>,
    Json(body): Json<MarkAllBody>,
) -> Json<serde_json::Value> {
    let mut s = st.lock().unwrap();
    let n = s.folders.len();
    for f in s.folders.iter_mut() {
        f.keep = body.keep;
    }
    Json(serde_json::json!({"ok": true, "marked": n}))
}

#[derive(Serialize)]
struct MoveResult {
    ok: bool,
    moved: usize,
    kept_folders: usize,
    errors: Vec<String>,
}

#[derive(Serialize)]
struct PreviewResult {
    ok: bool,
    kept_folders: usize,
    plan: Vec<MovePlanItem>,
    total_size: u64,
    errors: Vec<String>,
}

/// Build (but do NOT execute) the move plan. Same kept-folder guard as move.
async fn preview_move(State(st): State<SharedState>) -> Json<PreviewResult> {
    let s = st.lock().unwrap();
    let kept = s.folders.iter().filter(|f| f.keep).count();
    if kept == 0 && !s.folders.is_empty() {
        return Json(PreviewResult {
            ok: false,
            kept_folders: 0,
            plan: vec![],
            total_size: 0,
            errors: vec!["No folder is marked to keep — move blocked. Check at least one folder.".into()],
        });
    }
    let root = s.root.clone();
    let target = s.target.clone();
    let (plan, errors) = plan_move(&s.folders, &root, &target);
    let total_size = plan.iter().map(|p| p.size).sum();
    let kept_folders = kept;
    Json(PreviewResult {
        ok: errors.is_empty(),
        kept_folders,
        plan,
        total_size,
        errors,
    })
}

async fn move_marked(State(st): State<SharedState>) -> Json<MoveResult> {
    let mut s = st.lock().unwrap();
    let kept = s.folders.iter().filter(|f| f.keep).count();
    if kept == 0 && !s.folders.is_empty() {
        return Json(MoveResult {
            ok: false,
            moved: 0,
            kept_folders: 0,
            errors: vec!["No folder is marked to keep — move blocked. Check at least one folder.".into()],
        });
    }
    let root = s.root.clone();
    let target = s.target.clone();
    let (moved, errors) = move_non_kept(&mut s.folders, &root, &target);
    let kept_folders = s.folders.iter().filter(|f| f.keep).count();
    Json(MoveResult {
        ok: errors.is_empty(),
        moved,
        kept_folders,
        errors,
    })
}

/// Build the axum router with all routes wired to the shared state.
pub fn build_router(shared: SharedState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/vendor/:file", get(vendor_handler))
        .route("/api/state", get(get_state))
        .route("/api/scan_progress", get(scan_progress))
        .route("/api/mark_folder", post(mark_folder))
        .route("/api/mark_all", post(mark_all))
        .route("/api/move", post(move_marked))
        .route("/api/preview", post(preview_move))
        .with_state(shared)
}
