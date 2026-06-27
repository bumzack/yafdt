use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use glob::Pattern;
use mime_guess::from_path;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;
use walkdir::WalkDir;

type Md5 = String;

/// ======================
/// CLI
/// ======================

#[derive(Parser, Debug, Clone)]
#[command(name = "dupe_finder", about = "Find duplicate files and move them via a web UI")]
struct Cli {
    /// Root folder to search for duplicates.
    #[arg(long)]
    root: String,

    /// Target folder where non-kept duplicates get moved. Preserves full relative path.
    #[arg(long)]
    target: String,

    /// Minimum file size (e.g. 1, 1KB, 5MB, 100B). Default: no minimum.
    #[arg(long)]
    min_size: Option<String>,

    /// Include only files matching these glob patterns (e.g. '*.jpg'). Default: all.
    #[arg(long = "include", value_name = "GLOB")]
    include: Vec<String>,

    /// Ignore files whose names match any of these glob patterns (e.g. 'thumb*', '*.tmp').
    #[arg(long = "ignore-name", value_name = "GLOB")]
    ignore_names: Vec<String>,

    /// Ignore files with any of these extensions (e.g. 'log', 'tmp'). Leading dot optional.
    #[arg(long = "ignore-ext", value_name = "EXT")]
    ignore_exts: Vec<String>,

    /// Directory paths to skip (matched by prefix). Can be repeated.
    #[arg(long = "exclude-dir", value_name = "DIR")]
    exclude_dirs: Vec<String>,

    /// Address to serve the web UI on.
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,

    /// Do not open a browser automatically.
    #[arg(long)]
    no_browser: bool,
}

fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, suffix) = match s.find(|c: char| c.is_alphabetic()) {
        Some(i) => (&s[..i], s[i..].to_ascii_lowercase()),
        None => (s, String::new()),
    };
    let n: f64 = num.trim().parse().ok()?;
    let mult = match suffix.as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((n * mult) as u64)
}

/// ======================
/// SCAN
/// ======================

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FileInfo {
    path: PathBuf,
    size: u64,
    keep: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DuplicateGroup {
    hash: String,
    files: Vec<FileInfo>,
}

#[derive(Debug, Clone)]
struct ScanFilter {
    min_size: u64,
    include_globs: Vec<Pattern>,
    ignore_name_globs: Vec<Pattern>,
    ignore_exts: Vec<String>,
    exclude_dirs: Vec<PathBuf>,
}

impl ScanFilter {
    fn accepts(&self, path: &Path, meta: &fs::Metadata) -> bool {
        if meta.len() < self.min_size {
            return false;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            for g in &self.ignore_name_globs {
                if g.matches(name) {
                    return false;
                }
            }
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext = ext.to_ascii_lowercase();
            for ie in &self.ignore_exts {
                if ext == *ie {
                    return false;
                }
            }
        } else if !self.ignore_exts.is_empty() {
            // has no extension but extensions are ignored -> keep (only filter when ext present)
        }
        if !self.include_globs.is_empty() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let mut matched = false;
            for g in &self.include_globs {
                if g.matches(name) {
                    matched = true;
                    break;
                }
            }
            if !matched {
                return false;
            }
        }
        true
    }
}

fn md5sum(path: &Path) -> io::Result<Md5> {
    let mut file = fs::File::open(path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(format!("{:x}", md5::compute(buf)))
}

fn scan(root: &Path, filter: &ScanFilter) -> Vec<DuplicateGroup> {
    let mut by_hash: HashMap<Md5, Vec<FileInfo>> = HashMap::new();
    let mut scanned = 0usize;

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        // directory exclusion
        let mut excluded = false;
        for d in &filter.exclude_dirs {
            if p.starts_with(d) {
                excluded = true;
                break;
            }
        }
        if excluded {
            continue;
        }

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !filter.accepts(p, &meta) {
            continue;
        }

        scanned += 1;
        if scanned % 100 == 0 {
            println!("scanned {} files\u{2026}", scanned);
        }

        let hash = match md5sum(p) {
            Ok(h) => h,
            Err(e) => {
                println!("skip (hash error {:?}): {:?}", e, p);
                continue;
            }
        };
        by_hash
            .entry(hash)
            .or_insert_with(Vec::new)
            .push(FileInfo {
                path: p.to_path_buf(),
                size: meta.len(),
                keep: false,
            });
    }

    println!("scanned {} files.", scanned);

    // Keep only groups with >1 file (duplicates).
    let mut groups: Vec<DuplicateGroup> = by_hash
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(hash, mut files)| {
            // Sort by path so the first occurrence (canonical) is at top; default keep=the first.
            files.sort_by(|a, b| a.path.cmp(&b.path));
            if !files.is_empty() {
                files[0].keep = true; // default: keep one copy
            }
            DuplicateGroup { hash, files }
        })
        .collect();
    groups.sort_by(|a, b| b.files[0].size.cmp(&a.files[0].size));
    groups
}

/// ======================
/// VIEW MODEL for the web UI
/// ======================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FolderView {
    folder: String,
    /// file paths (duplicate copies) that live in this folder, grouped by group hash
    groups: Vec<FolderGroupEntry>,
    /// true if every file in this folder (across its groups) is marked keep
    all_kept: bool,
    /// how many duplicate copies live in this folder
    dup_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FolderGroupEntry {
    group_hash: String,
    path: String,
    size: u64,
    keep: bool,
    original: String,
}

fn build_folder_views(groups: &[DuplicateGroup], root: &Path) -> Vec<FolderView> {
    // For each duplicate copy, bucket by parent folder. A folder appears in the
    // list if it contains at least one duplicate copy.
    let mut by_folder: HashMap<PathBuf, Vec<(usize, usize)>> = HashMap::new(); // folder -> Vec<(group_idx, file_idx)>
    for (gi, g) in groups.iter().enumerate() {
        for (fi, f) in g.files.iter().enumerate() {
            if let Some(parent) = f.path.parent() {
                by_folder
                    .entry(parent.to_path_buf())
                    .or_default()
                    .push((gi, fi));
            }
        }
    }

    let mut views: Vec<FolderView> = by_folder
        .into_iter()
        .map(|(folder, idxs)| {
            let mut entries = Vec::with_capacity(idxs.len());
            let mut all_kept = true;
            for (gi, fi) in &idxs {
                let f = &groups[*gi].files[*fi];
                if !f.keep {
                    all_kept = false;
                }
                // original = first file of the group (lowest path)
                let original = groups[*gi]
                    .files
                    .first()
                    .map(|x| x.path.to_string_lossy().to_string())
                    .unwrap_or_default();
                entries.push(FolderGroupEntry {
                    group_hash: groups[*gi].hash.clone(),
                    path: f.path.to_string_lossy().to_string(),
                    size: f.size,
                    keep: f.keep,
                    original,
                });
            }
            // sort entries by path for stable display
            entries.sort_by(|a, b| a.path.cmp(&b.path));
            let dup_count = entries.len();
            FolderView {
                folder: folder.to_string_lossy().to_string(),
                groups: entries,
                all_kept,
                dup_count,
            }
        })
        .collect();

    views.sort_by(|a, b| a.folder.cmp(&b.folder));

    // If root is given, also annotate nothing else - we keep folder path absolute.
    let _ = root;
    views
}

/// ======================
/// APP STATE
/// ======================

#[derive(Debug)]
struct AppState {
    root: PathBuf,
    target: PathBuf,
    groups: Vec<DuplicateGroup>,
    folder_views: Vec<FolderView>,
}

type SharedState = Arc<Mutex<AppState>>;

fn rebuild_folder_views(st: &mut AppState) {
    st.folder_views = build_folder_views(&st.groups, &st.root);
}

/// ======================
/// MOVE
/// ======================

fn move_non_kept(groups: &mut Vec<DuplicateGroup>, root: &Path, target: &Path) -> io::Result<usize> {
    let mut moved = 0;
    for g in groups.iter_mut() {
        // safety: each group must keep at least one file
        if !g.files.iter().any(|f| f.keep) {
            println!("group {} has no kept file; skipping move for this group", g.hash);
            continue;
        }
        for f in g.files.iter_mut().filter(|f| !f.keep) {
            let rel = match f.path.strip_prefix(root) {
                Ok(r) => r,
                Err(_) => {
                    println!(
                        "skip (not under root): {:?}",
                        f.path
                    );
                    continue;
                }
            };
            let dest = target.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            println!("moving ||{:?}||  -->  ||{:?}||", f.path, dest);
            // Prefer rename; fall back to copy+delete across volumes.
            if let Err(e) = fs::rename(&f.path, &dest) {
                if e.raw_os_error() == Some(18) /* EXDEV */ || e.kind() == io::ErrorKind::CrossesDevices {
                    fs::copy(&f.path, &dest)?;
                    fs::remove_file(&f.path)?;
                } else {
                    return Err(e);
                }
            }
            f.keep = true; // it's gone now
            moved += 1;
        }
    }
    // Prune moved files from groups so UI stays consistent.
    for g in groups.iter_mut() {
        g.files.retain(|f| Path::new(&f.path).exists());
    }
    groups.retain(|g| g.files.len() > 1);
    Ok(moved)
}

/// ======================
/// EMBEDDED STATIC
/// ======================

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

async fn index_handler() -> impl IntoResponse {
    serve_asset("index.html").unwrap_or_else(|| {
        (StatusCode::NOT_FOUND, "index.html not found").into_response()
    })
}

/// ======================
/// HANDLERS
/// ======================

#[derive(Serialize)]
struct StateResponse {
    root: String,
    target: String,
    groups_count: usize,
    total_dupes: usize,
    folders: Vec<FolderView>,
    groups: Vec<DuplicateGroup>,
    /// how many groups currently have zero kept files (UI should block move)
    ambiguous: usize,
}

async fn get_state(State(st): State<SharedState>) -> Json<StateResponse> {
    let s = st.lock().await;
    let total_dupes: usize = s.groups.iter().map(|g| g.files.len()).sum();
    let ambiguous = s
        .groups
        .iter()
        .filter(|g| !g.files.iter().any(|f| f.keep))
        .count();
    Json(StateResponse {
        root: s.root.to_string_lossy().to_string(),
        target: s.target.to_string_lossy().to_string(),
        groups_count: s.groups.len(),
        total_dupes,
        folders: s.folder_views.clone(),
        groups: s.groups.clone(),
        ambiguous,
    })
}

#[derive(Deserialize)]
struct MarkFileBody {
    path: String,
    keep: bool,
}

async fn mark_file(
    State(st): State<SharedState>,
    Json(body): Json<MarkFileBody>,
) -> Json<serde_json::Value> {
    let mut s = st.lock().await;
    let target = fs::canonicalize(&body.path).unwrap_or_else(|_| PathBuf::from(&body.path));
    let target_str = target.to_string_lossy().to_string();
    let mut changed = false;
    for g in s.groups.iter_mut() {
        for f in g.files.iter_mut() {
            let fp = fs::canonicalize(&f.path).unwrap_or_else(|_| f.path.clone());
            if fp.to_string_lossy() == target_str {
                f.keep = body.keep;
                changed = true;
            }
        }
    }
    if changed {
        rebuild_folder_views(&mut s);
    }
    Json(serde_json::json!({"ok": changed}))
}

#[derive(Deserialize)]
struct MarkGroupBody {
    group_hash: String,
    keep_path: String,
}

/// Mark only the given path as keep in a group, un-keep all others in that group.
async fn keep_only(
    State(st): State<SharedState>,
    Json(body): Json<MarkGroupBody>,
) -> Json<serde_json::Value> {
    let mut s = st.lock().await;
    let mut found = false;
    for g in s.groups.iter_mut() {
        if g.hash != body.group_hash {
            continue;
        }
        found = true;
        for f in g.files.iter_mut() {
            f.keep = f.path.to_string_lossy() == body.keep_path;
        }
    }
    if found {
        rebuild_folder_views(&mut s);
    }
    Json(serde_json::json!({"ok": found}))
}

#[derive(Deserialize)]
struct MarkFolderBody {
    folder: String,
    keep: bool,
    /// if true, also mark all files in subfolders of `folder`
    recursive: bool,
}

async fn mark_folder(
    State(st): State<SharedState>,
    Json(body): Json<MarkFolderBody>,
) -> Json<serde_json::Value> {
    let mut s = st.lock().await;
    let folder = fs::canonicalize(&body.folder).unwrap_or_else(|_| PathBuf::from(&body.folder));
    let mut count = 0;
    for g in s.groups.iter_mut() {
        for f in g.files.iter_mut() {
            let parent = f.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let parent_canon = fs::canonicalize(&parent).unwrap_or(parent.clone());
            let matches = if body.recursive {
                parent_canon.starts_with(&folder)
            } else {
                parent_canon == folder
            };
            if matches {
                f.keep = body.keep;
                count += 1;
            }
        }
    }
    if count > 0 {
        rebuild_folder_views(&mut s);
    }
    Json(serde_json::json!({"ok": true, "marked": count}))
}

#[derive(Serialize)]
struct MoveResult {
    ok: bool,
    moved: usize,
    ambiguous: usize,
}

async fn move_marked(State(st): State<SharedState>) -> Json<MoveResult> {
    let mut s = st.lock().await;
    let ambiguous = s
        .groups
        .iter()
        .filter(|g| !g.files.iter().any(|f| f.keep))
        .count();
    if ambiguous > 0 {
        return Json(MoveResult {
            ok: false,
            moved: 0,
            ambiguous,
        });
    }
    let root = s.root.clone();
    let target = s.target.clone();
    let res = move_non_kept(&mut s.groups, &root, &target);
    match res {
        Ok(n) => {
            rebuild_folder_views(&mut s);
            Json(MoveResult {
                ok: true,
                moved: n,
                ambiguous: 0,
            })
        }
        Err(e) => {
            println!("move error: {:?}", e);
            Json(MoveResult {
                ok: false,
                moved: 0,
                ambiguous: 0,
            })
        }
    }
}

async fn reset_keep(State(st): State<SharedState>) -> Json<serde_json::Value> {
    let mut s = st.lock().await;
    for g in s.groups.iter_mut() {
        for f in g.files.iter_mut() {
            f.keep = false;
        }
        if let Some(first) = g.files.first_mut() {
            first.keep = true;
        }
    }
    rebuild_folder_views(&mut s);
    Json(serde_json::json!({"ok": true}))
}

/// ======================
/// MAIN
/// ======================

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let root = fs::canonicalize(&cli.root).unwrap_or_else(|e| {
        eprintln!("invalid --root '{}': {}", cli.root, e);
        std::process::exit(1);
    });
    if !root.is_dir() {
        eprintln!("--root '{}' is not a directory", root.display());
        std::process::exit(1);
    }
    let target = PathBuf::from(&cli.target);
    fs::create_dir_all(&target).expect("could not create --target");

    let min_size = cli
        .min_size
        .as_deref()
        .and_then(parse_bytes)
        .unwrap_or(0);

    let include_globs = cli
        .include
        .iter()
        .filter_map(|s| match Pattern::new(s) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("invalid --include glob '{}': {}", s, e);
                None
            }
        })
        .collect::<Vec<_>>();
    let ignore_name_globs = cli
        .ignore_names
        .iter()
        .filter_map(|s| match Pattern::new(s) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("invalid --ignore-name glob '{}': {}", s, e);
                None
            }
        })
        .collect::<Vec<_>>();
    let ignore_exts = cli
        .ignore_exts
        .iter()
        .map(|s| s.trim_start_matches('.').to_ascii_lowercase())
        .collect::<Vec<_>>();
    let exclude_dirs = cli
        .exclude_dirs
        .iter()
        .map(|s| fs::canonicalize(s).unwrap_or_else(|_| PathBuf::from(s)))
        .collect::<Vec<_>>();

    let filter = ScanFilter {
        min_size,
        include_globs,
        ignore_name_globs,
        ignore_exts,
        exclude_dirs,
    };

    println!("Scanning root : {}", root.display());
    println!("Target folder : {}", target.display());
    println!("Min size      : {} bytes", filter.min_size);
    println!("Include globs : {:?}", cli.include);
    println!("Ignore names  : {:?}", cli.ignore_names);
    println!("Ignore exts   : {:?}", filter.ignore_exts);
    println!("Exclude dirs  : {:?}", filter.exclude_dirs);
    println!("Scanning\u{2026}");

    let groups = scan(&root, &filter);
    println!(
        "Found {} duplicate group(s).",
        groups.len()
    );

    let folder_views = build_folder_views(&groups, &root);
    println!(
        "{} folder(s) contain duplicates.",
        folder_views.len()
    );

    let state = AppState {
        root,
        target,
        groups,
        folder_views,
    };
    let shared = Arc::new(Mutex::new(state));

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/api/state", get(get_state))
        .route("/api/mark_file", post(mark_file))
        .route("/api/mark_folder", post(mark_folder))
        .route("/api/keep_only", post(keep_only))
        .route("/api/move", post(move_marked))
        .route("/api/reset_keep", post(reset_keep))
        .with_state(shared);

    let bind: SocketAddr = cli
        .bind
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("invalid --bind '{}': {}", cli.bind, e);
            std::process::exit(1);
        });
    println!("\nServing web UI on http://{}", bind);

    if !cli.no_browser {
        let url = format!("http://{}", bind);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            if let Err(e) = webbrowser::open(&url) {
                println!("failed to open browser: {}", e);
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(bind).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}