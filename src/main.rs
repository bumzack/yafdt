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
    io::{self},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
};
use walkdir::WalkDir;

type Md5 = String;

/// ======================
/// USER PREFERENCES (~/.config/dupe_finder/prefs.json etc.)
/// ======================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Prefs {
    /// Directory name fragments to always exclude (matched against any path
    /// component). Sensible defaults: node_modules, target, .git, ...
    #[serde(default = "Prefs::default_exclude_components")]
    exclude_components: Vec<String>,
    /// Absolute directory paths to always skip (prefix match).
    #[serde(default)]
    exclude_dirs: Vec<String>,
    /// Default min file size as a human string ("1MB", ...). Empty = no minimum.
    #[serde(default)]
    min_size: Option<String>,
    /// Always ignore files whose names match these globs.
    #[serde(default)]
    ignore_names: Vec<String>,
    /// Always ignore files with these extensions.
    #[serde(default)]
    ignore_exts: Vec<String>,
    /// Default include globs (empty = all files).
    #[serde(default)]
    include: Vec<String>,
    /// Bind address for the web UI.
    #[serde(default = "Prefs::default_bind")]
    bind: String,
    /// Open the browser automatically.
    #[serde(default = "Prefs::default_open_browser")]
    open_browser: bool,
}

impl Prefs {
    fn default_exclude_components() -> Vec<String> {
        vec![
            "node_modules".into(),
            "target".into(),
            ".git".into(),
            ".svn".into(),
            ".hg".into(),
            "vendor".into(),
            "__pycache__".into(),
            ".venv".into(),
            "venv".into(),
            ".next".into(),
            ".nuxt".into(),
            ".cache".into(),
            ".Trash".into(),
        ]
    }
    fn default_bind() -> String { "127.0.0.1:8787".into() }
    fn default_open_browser() -> bool { true }

    fn config_path() -> Option<PathBuf> {
        let base = dirs::config_dir()?;
        Some(base.join("dupe_finder").join("prefs.json"))
    }

    fn load() -> Prefs {
        let Some(path) = Self::config_path() else {
            return Prefs::default_values();
        };
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<Prefs>(&s).unwrap_or_else(|e| {
                eprintln!(
                    "warning: failed to parse prefs at {}: {} — using defaults",
                    path.display(),
                    e
                );
                Prefs::default_values()
            }),
            Err(_) => Prefs::default_values(),
        }
    }

    fn default_values() -> Prefs {
        Prefs {
            exclude_components: Self::default_exclude_components(),
            exclude_dirs: Vec::new(),
            min_size: None,
            ignore_names: Vec::new(),
            ignore_exts: Vec::new(),
            include: Vec::new(),
            bind: Self::default_bind(),
            open_browser: Self::default_open_browser(),
        }
    }

    fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::config_path() else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no config dir"));
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let s = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&path, s)?;
        println!("saved preferences to {}", path.display());
        Ok(())
    }
}

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

    /// Directory name fragments to always skip (matched against any path
    /// component, e.g. 'node_modules', 'target'). Defaults come from prefs.
    #[arg(long = "exclude-component", value_name = "NAME")]
    exclude_components: Vec<String>,

    /// Address to serve the web UI on. Defaults from prefs (127.0.0.1:8787).
    #[arg(long)]
    bind: Option<String>,

    /// Do not open a browser automatically.
    #[arg(long)]
    no_browser: bool,

    /// Launch the native egui GUI instead of the web UI (requires the `gui` feature).
    #[arg(long)]
    gui: bool,

    /// Save the current flags (exclude-components, exclude-dirs, min-size, ignore-*,
    /// include, bind, open-browser) to the user prefs file and exit.
    #[arg(long)]
    save_prefs: bool,

    /// Ignore the user prefs file entirely for this run.
    #[arg(long)]
    no_prefs: bool,

    /// Don't move anything. Print what *would* be moved (src \u2192 dest, total
    /// bytes) to the terminal and exit. Useful as a pre-flight check.
    #[arg(long)]
    dry_run: bool,
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

/// Human-readable byte count for terminal output.
fn human_bytes(b: u64) -> String {
    if b >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", b as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if b >= 1024 * 1024 {
        format!("{:.2} MB", b as f64 / 1024.0 / 1024.0)
    } else if b >= 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{} B", b)
    }
}

/// ======================
/// SCAN
/// ======================

/// A single duplicate file copy. Display-only metadata; keep is per-FOLDER,
/// not per-file, so there's no keep flag here.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct FileEntry {
    path: String,
    size: u64,
}

/// A folder that contains at least one duplicate file copy. The user marks
/// whole folders as "keep in place" (keep=true); unchecked folders' files
/// get moved to target.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Folder {
    folder: String,
    keep: bool,
    files: Vec<FileEntry>,
    /// total bytes of duplicate files in this folder
    total_size: u64,
}

#[derive(Debug, Clone)]
struct ScanFilter {
    min_size: u64,
    include_globs: Vec<Pattern>,
    ignore_name_globs: Vec<Pattern>,
    ignore_exts: Vec<String>,
    exclude_dirs: Vec<PathBuf>,
    /// Directory name fragments to skip if any path component equals one of
    /// these (e.g. "node_modules", "target").
    exclude_components: Vec<String>,
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

    /// True if the path lives inside an excluded dir or contains an excluded
    /// component name.
    fn excluded(&self, p: &Path) -> bool {
        for d in &self.exclude_dirs {
            if p.starts_with(d) {
                return true;
            }
        }
        if !self.exclude_components.is_empty() {
            for comp in p.components() {
                if let std::path::Component::Normal(os) = comp {
                    if let Some(name) = os.to_str() {
                        if self.exclude_components.iter().any(|c| c == name) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

fn md5sum(path: &Path) -> io::Result<Md5> {
    // Stream the file through a BufReader into the md5 Context (which impls
    // io::Write). O(1) memory regardless of file size, instead of slurping the
    // whole file into a Vec.
    let file = fs::File::open(path)?;
    let mut reader = io::BufReader::with_capacity(64 * 1024, file);
    let mut ctx = md5::Context::new();
    io::copy(&mut reader, &mut ctx)?;
    Ok(format!("{:x}", ctx.compute()))
}

/// Scan `root`, hash files, and build a list of folders that contain duplicate
/// files. A folder appears in the result only if at least one file in it has
/// the same content (hash) as some file elsewhere.
///
/// Two-pass for performance:
///   1. Walk + filter + bucket every accepted file by size (cheap, no I/O reads).
///   2. Only hash files whose size bucket has >1 entry. Unique-size files can
///      never be duplicates, so they skip hashing entirely.
///
/// `walked` / `hashed` are atomics updated as the scan progresses, so a web UI
/// can poll them for a live counter. Pass dummy atomics if you don't care.
fn scan_with_progress(
    root: &Path,
    filter: &ScanFilter,
    walked: &AtomicUsize,
    hashed: &AtomicUsize,
) -> Vec<Folder> {
    // Pass 1: collect (path, size) for every accepted file, grouped by size.
    let mut by_size: HashMap<u64, Vec<(PathBuf, u64)>> = HashMap::new();
    let mut scanned = 0usize;

    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if filter.excluded(p) {
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
        walked.store(scanned, Ordering::Relaxed);
        if scanned % 1000 == 0 {
            println!("walked {} files\u{2026}", scanned);
        }
        by_size
            .entry(meta.len())
            .or_insert_with(Vec::new)
            .push((p.to_path_buf(), meta.len()));
    }

    println!(
        "walked {} files, {} distinct size bucket(s).",
        scanned,
        by_size.len()
    );

    // Pass 2: hash only files in size buckets with >1 entry.
    let mut hashed_n = 0usize;
    let mut by_hash: HashMap<Md5, Vec<(PathBuf, u64)>> = HashMap::new();
    for (_, files) in by_size.into_iter().filter(|(_, v)| v.len() > 1) {
        for (path, size) in files {
            hashed_n += 1;
            hashed.store(hashed_n, Ordering::Relaxed);
            if hashed_n % 100 == 0 {
                println!("hashed {} files\u{2026}", hashed_n);
            }
            let hash = match md5sum(&path) {
                Ok(h) => h,
                Err(e) => {
                    println!("skip (hash error {:?}): {:?}", e, path);
                    continue;
                }
            };
            by_hash
                .entry(hash)
                .or_insert_with(Vec::new)
                .push((path, size));
        }
    }

    println!("hashed {} files (size prefilter skipped the rest).", hashed_n);

    // Keep only hashes with >1 file (duplicates), then bucket every duplicate
    // copy by its parent folder.
    let mut by_folder: HashMap<PathBuf, Vec<FileEntry>> = HashMap::new();
    for files in by_hash.into_values().filter(|v| v.len() > 1) {
        for (path, size) in files {
            let parent = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            by_folder
                .entry(parent)
                .or_insert_with(Vec::new)
                .push(FileEntry {
                    path: path.to_string_lossy().to_string(),
                    size,
                });
        }
    }

    let mut folders: Vec<Folder> = by_folder
        .into_iter()
        .map(|(folder, mut files)| {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            let total_size = files.iter().map(|f| f.size).sum();
            Folder {
                folder: folder.to_string_lossy().to_string(),
                keep: false,
                files,
                total_size,
            }
        })
        .collect();
    folders.sort_by(|a, b| a.folder.cmp(&b.folder));
    folders
}

/// Synchronous scan wrapper for callers that don't care about live progress
/// (tests, --dry-run, --gui). Uses throwaway atomics.
fn scan(root: &Path, filter: &ScanFilter) -> Vec<Folder> {
    let walked = AtomicUsize::new(0);
    let hashed = AtomicUsize::new(0);
    scan_with_progress(root, filter, &walked, &hashed)
}

/// ======================
/// APP STATE
/// ======================

#[derive(Debug)]
struct AppState {
    root: PathBuf,
    target: PathBuf,
    folders: Vec<Folder>,
    /// true while the background scan thread is still running.
    scanning: bool,
    /// set if the scan thread panicked / errored.
    scan_error: Option<String>,
    /// live counters (also written by the scan thread; read by /api/scan_progress)
    walked: Arc<AtomicUsize>,
    hashed: Arc<AtomicUsize>,
}

type SharedState = Arc<Mutex<AppState>>;

/// ======================
/// MOVE
/// ======================

/// One planned move: source file -> destination under target, preserving the
/// full relative path from root.
#[derive(Debug, Clone, Serialize)]
struct MovePlanItem {
    src: String,
    dest: String,
    size: u64,
}

/// Build the list of files that *would* be moved, without touching the disk.
/// `folders` is read-only here. The kept-folder guard (>=1 kept) is the
/// caller's responsibility.
fn plan_move(folders: &[Folder], root: &Path, target: &Path) -> (Vec<MovePlanItem>, Vec<String>) {
    let mut plan: Vec<MovePlanItem> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for fv in folders {
        if fv.keep {
            continue;
        }
        for f in &fv.files {
            let p = Path::new(&f.path);
            let rel = match p.strip_prefix(root) {
                Ok(r) => r,
                Err(_) => {
                    errors.push(format!("not under root: {}", f.path));
                    continue;
                }
            };
            let dest = target.join(rel);
            plan.push(MovePlanItem {
                src: f.path.clone(),
                dest: dest.to_string_lossy().to_string(),
                size: f.size,
            });
        }
    }
    (plan, errors)
}

/// Execute a previously built plan: move each src to its dest, creating parent
/// dirs as needed. Returns (moved_count, per_file_errors).
fn execute_move(plan: &[MovePlanItem]) -> (usize, Vec<String>) {
    let mut moved = 0;
    let mut errors: Vec<String> = Vec::new();
    for item in plan {
        let p = Path::new(&item.src);
        let dest = Path::new(&item.dest);
        if let Some(parent) = dest.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                errors.push(format!("mkdir {} failed: {}", parent.display(), e));
                continue;
            }
        }
        println!("moving ||{}||  -->  ||{}||", p.display(), dest.display());
        if let Err(e) = fs::rename(p, dest) {
            if e.raw_os_error() == Some(18) /* EXDEV */
                || e.kind() == io::ErrorKind::CrossesDevices
            {
                if let Err(e2) = fs::copy(p, dest).and_then(|_| fs::remove_file(p)) {
                    errors.push(format!("copy {} failed: {}", item.src, e2));
                    continue;
                }
            } else {
                errors.push(format!("rename {} failed: {}", item.src, e));
                continue;
            }
        }
        moved += 1;
    }
    (moved, errors)
}

/// Move every file in every non-kept folder to target/<rel path from root>.
/// Returns (moved, errors). Errors are per-file so one bad file doesn't abort
/// the whole batch. Also prunes moved files from `folders` so the UI updates.
fn move_non_kept(
    folders: &mut Vec<Folder>,
    root: &Path,
    target: &Path,
) -> (usize, Vec<String>) {
    let (plan, mut errors) = plan_move(folders, root, target);
    let (moved, exec_errors) = execute_move(&plan);
    errors.extend(exec_errors);

    // Clear files in non-kept folders (they've been moved); keep kept folders as-is.
    for fv in folders.iter_mut() {
        if !fv.keep {
            fv.files.clear();
            fv.total_size = 0;
        }
    }
    folders.retain(|f| !f.files.is_empty() || f.keep);
    (moved, errors)
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

/// Serve a vendored static asset: GET /vendor/bootstrap.min.css etc.
/// Files live under src/static/vendor/ and are embedded by rust-embed.
async fn vendor_handler(
    axum::extract::Path(file): axum::extract::Path<String>,
) -> impl IntoResponse {
    let asset_path = format!("vendor/{}", file);
    serve_asset(&asset_path).unwrap_or_else(|| {
        (StatusCode::NOT_FOUND, format!("vendor asset not found: {}", file)).into_response()
    })
}

/// ======================
/// HANDLERS
/// ======================

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
        walked: s.walked.load(Ordering::Relaxed),
        hashed: s.hashed.load(Ordering::Relaxed),
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

/// ======================
/// MAIN
/// ======================

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // ---- preferences ----
    let prefs = if cli.no_prefs {
        Prefs::default_values()
    } else {
        Prefs::load()
    };

    // --save-prefs: persist the *effective* preferences (CLI merged onto loaded
    // prefs as a union for lists; CLI wins for scalars) and exit.
    // root/target are never stored.
    if cli.save_prefs {
        let mut to_save = prefs.clone();
        fn union(a: &[String], b: &[String]) -> Vec<String> {
            let mut out = a.to_vec();
            for x in b {
                if !out.contains(x) {
                    out.push(x.clone());
                }
            }
            out
        }
        if !cli.exclude_components.is_empty() {
            to_save.exclude_components = union(&to_save.exclude_components, &cli.exclude_components);
        }
        if !cli.exclude_dirs.is_empty() {
            to_save.exclude_dirs = union(&to_save.exclude_dirs, &cli.exclude_dirs);
        }
        if !cli.ignore_names.is_empty() {
            to_save.ignore_names = union(&to_save.ignore_names, &cli.ignore_names);
        }
        if !cli.ignore_exts.is_empty() {
            to_save.ignore_exts = union(&to_save.ignore_exts, &cli.ignore_exts);
        }
        if !cli.include.is_empty() {
            to_save.include = union(&to_save.include, &cli.include);
        }
        if cli.min_size.is_some() {
            to_save.min_size = cli.min_size.clone();
        }
        if let Some(b) = &cli.bind {
            to_save.bind = b.clone();
        }
        to_save.open_browser = !cli.no_browser;
        if let Err(e) = to_save.save() {
            eprintln!("failed to save prefs: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // ---- root / target (job-specific, never from prefs) ----
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

    // ---- merge CLI flags with prefs (union for lists; CLI wins for scalars) ----
    let min_size = cli
        .min_size
        .as_deref()
        .or(prefs.min_size.as_deref())
        .and_then(parse_bytes)
        .unwrap_or(0);

    let include_raw: Vec<String> = cli
        .include
        .iter()
        .chain(prefs.include.iter())
        .cloned()
        .collect();
    let include_globs = include_raw
        .iter()
        .filter_map(|s| match Pattern::new(s) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("invalid --include glob '{}': {}", s, e);
                None
            }
        })
        .collect::<Vec<_>>();

    let ignore_names_raw: Vec<String> = cli
        .ignore_names
        .iter()
        .chain(prefs.ignore_names.iter())
        .cloned()
        .collect();
    let ignore_name_globs = ignore_names_raw
        .iter()
        .filter_map(|s| match Pattern::new(s) {
            Ok(p) => Some(p),
            Err(e) => {
                eprintln!("invalid --ignore-name glob '{}': {}", s, e);
                None
            }
        })
        .collect::<Vec<_>>();

    let ignore_exts: Vec<String> = cli
        .ignore_exts
        .iter()
        .chain(prefs.ignore_exts.iter())
        .map(|s| s.trim_start_matches('.').to_ascii_lowercase())
        .collect();

    let exclude_dirs: Vec<PathBuf> = cli
        .exclude_dirs
        .iter()
        .chain(prefs.exclude_dirs.iter())
        .map(|s| fs::canonicalize(s).unwrap_or_else(|_| PathBuf::from(s)))
        .collect();

    let exclude_components: Vec<String> = cli
        .exclude_components
        .iter()
        .chain(prefs.exclude_components.iter())
        .cloned()
        .collect();

    let filter = ScanFilter {
        min_size,
        include_globs,
        ignore_name_globs,
        ignore_exts,
        exclude_dirs,
        exclude_components,
    };

    println!("Scanning root  : {}", root.display());
    println!("Target folder  : {}", target.display());
    println!("Min size       : {} bytes", filter.min_size);
    println!("Include globs  : {}", include_raw.join(", "));
    println!("Ignore names   : {}", ignore_names_raw.join(", "));
    println!("Ignore exts    : {}", filter.ignore_exts.join(", "));
    println!(
        "Exclude dirs   : {}",
        filter
            .exclude_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("Exclude parts  : {}", filter.exclude_components.join(", "));
    println!("Scanning\u{2026}");

    // --gui path: scan synchronously, then launch the native window. The egui
    // app reads AppState.folders directly, so the scan must finish first.
    #[cfg(feature = "gui")]
    if cli.gui {
        let folders = scan(&root, &filter);
        println!(
            "Found {} folder(s) containing duplicates.",
            folders.len()
        );
        let walked = Arc::new(AtomicUsize::new(0));
        let hashed = Arc::new(AtomicUsize::new(0));
        let state = AppState {
            root,
            target,
            folders,
            scanning: false,
            scan_error: None,
            walked,
            hashed,
        };
        let shared = Arc::new(Mutex::new(state));
        run_gui(shared);
        return;
    }
    #[cfg(not(feature = "gui"))]
    if cli.gui {
        eprintln!("--gui requires building with the `gui` feature: cargo run --features gui -- --gui");
        std::process::exit(1);
    }

    // --dry-run: scan synchronously, print the move plan, exit. No server.
    if cli.dry_run {
        let folders = scan(&root, &filter);
        println!(
            "Found {} folder(s) containing duplicates.",
            folders.len()
        );
        println!("\n=== DRY RUN (no folders kept — everything would move) ===");
        let (plan, errors) = plan_move(&folders, &root, &target);
        let total: u64 = plan.iter().map(|p| p.size).sum();
        for item in &plan {
            println!("  {}  -->  {}", item.src, item.dest);
        }
        println!(
            "\n{} file(s) would move, {} total ({}).",
            plan.len(),
            total,
            human_bytes(total)
        );
        if !errors.is_empty() {
            println!("errors:");
            for e in &errors {
                println!("  {}", e);
            }
        }
        println!("=== end dry run ===");
        return;
    }

    // Web path: start the server immediately with scanning=true and empty
    // folders, spawn the scan on a background thread, and let the browser poll
    // /api/scan_progress until it's done. The thread deposits the folders and
    // flips scanning=false when finished.
    let walked = Arc::new(AtomicUsize::new(0));
    let hashed = Arc::new(AtomicUsize::new(0));
    let state = AppState {
        root: root.clone(),
        target: target.clone(),
        folders: Vec::new(),
        scanning: true,
        scan_error: None,
        walked: walked.clone(),
        hashed: hashed.clone(),
    };
    let shared = Arc::new(Mutex::new(state));

    {
        let shared = shared.clone();
        let root = root.clone();
        let filter = filter.clone();
        let walked = walked.clone();
        let hashed = hashed.clone();
        thread::spawn(move || {
            let folders = scan_with_progress(&root, &filter, &walked, &hashed);
            println!(
                "Found {} folder(s) containing duplicates.",
                folders.len()
            );
            let mut s = shared.lock().unwrap();
            s.folders = folders;
            s.scanning = false;
        });
    }

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/index.html", get(index_handler))
        .route("/vendor/:file", get(vendor_handler))
        .route("/api/state", get(get_state))
        .route("/api/scan_progress", get(scan_progress))
        .route("/api/mark_folder", post(mark_folder))
        .route("/api/mark_all", post(mark_all))
        .route("/api/move", post(move_marked))
        .route("/api/preview", post(preview_move))
        .with_state(shared);

    let bind_str = cli.bind.clone().unwrap_or_else(|| prefs.bind.clone());
    let bind: SocketAddr = bind_str
        .parse()
        .unwrap_or_else(|e| {
            eprintln!("invalid --bind '{}': {}", bind_str, e);
            std::process::exit(1);
        });
    println!("\nServing web UI on http://{}", bind);

    let open_browser = !cli.no_browser && prefs.open_browser;
    if open_browser {
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

/// ======================
/// NATIVE GUI (egui, behind the `gui` feature)
/// ======================

#[cfg(feature = "gui")]
mod gui {
    use super::*;
    use eframe::egui;

    pub fn run_gui(shared: SharedState) {
        let opts = eframe::NativeOptions::default();
        let _ = eframe::run_native(
            "Duplicate Finder",
            opts,
            Box::new(|_cc| Ok(Box::new(GuiApp { shared }))),
        );
    }

    struct GuiApp {
        shared: SharedState,
    }

    impl eframe::App for GuiApp {
        fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
            let mut s = self.shared.lock().unwrap();

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Duplicate Finder");
                ui.separator();
                ui.label(format!("Root  : {}", s.root.display()));
                ui.label(format!("Target: {}", s.target.display()));

                let kept = s.folders.iter().filter(|f| f.keep).count();
                let total_files: usize = s.folders.iter().map(|f| f.files.len()).sum();
                ui.add_space(4.0);
                ui.label(format!(
                    "{} folder(s) with duplicates \u{2022} {} file(s) \u{2022} {} folder(s) kept",
                    s.folders.len(),
                    total_files,
                    kept
                ));

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Keep all folders").clicked() {
                        for f in s.folders.iter_mut() {
                            f.keep = true;
                        }
                    }
                    if ui.button("Un-keep all folders").clicked() {
                        for f in s.folders.iter_mut() {
                            f.keep = false;
                        }
                    }
                    let move_enabled = kept > 0 || s.folders.is_empty();
                    let clicked = ui
                        .add_enabled(move_enabled, egui::Button::new("Move non-kept"))
                        .clicked();
                    if clicked {
                        let root = s.root.clone();
                        let target = s.target.clone();
                        let (moved, errors) = move_non_kept(&mut s.folders, &root, &target);
                        println!("moved {} files, {} error(s)", moved, errors.len());
                        for e in &errors {
                            println!("  err: {}", e);
                        }
                    }
                });

                if kept == 0 && !s.folders.is_empty() {
                    ui.colored_label(
                        egui::Color32::RED,
                        "No folder is marked to keep \u{2014} move blocked.",
                    );
                }

                ui.separator();
                ui.label("Folders containing duplicates (check to keep in place):");

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut toggles: Vec<(usize, bool)> = Vec::new();
                    for (i, f) in s.folders.iter_mut().enumerate().take(5_000) {
                        let mut keep = f.keep;
                        let header = format!(
                            "{}  [{} file(s), {} B]",
                            f.folder,
                            f.files.len(),
                            f.total_size
                        );
                        ui.collapsing(header, |ui| {
                            if ui.checkbox(&mut keep, "Keep this folder + subfolders").changed() {
                                toggles.push((i, keep));
                            }
                            ui.separator();
                            for fe in &f.files {
                                ui.label(format!("{}  [{} B]", fe.path, fe.size));
                            }
                        });
                    }
                    // Apply toggles with a recursive cascade: keeping a folder
                    // also keeps all of its subfolders.
                    for (i, keep) in toggles {
                        let Some(target) = s.folders.get(i) else { continue };
                        let target_canon = fs::canonicalize(&target.folder)
                            .unwrap_or_else(|_| PathBuf::from(&target.folder));
                        for f in s.folders.iter_mut() {
                            let f_canon = fs::canonicalize(&f.folder)
                                .unwrap_or_else(|_| PathBuf::from(&f.folder));
                            if f_canon == target_canon || f_canon.starts_with(&target_canon) {
                                f.keep = keep;
                            }
                        }
                    }
                });
            });

            ctx.request_repaint();
        }
    }
}

#[cfg(feature = "gui")]
use gui::run_gui;

/// ======================
/// TESTS
/// ======================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ---- parse_bytes ----

    #[test]
    fn parse_bytes_plain_number() {
        assert_eq!(parse_bytes("1024"), Some(1024));
        assert_eq!(parse_bytes("0"), Some(0));
    }

    #[test]
    fn parse_bytes_b_suffix() {
        assert_eq!(parse_bytes("100B"), Some(100));
        assert_eq!(parse_bytes("100b"), Some(100));
        assert_eq!(parse_bytes("100 B"), Some(100));
    }

    #[test]
    fn parse_bytes_kb_mb_gb() {
        assert_eq!(parse_bytes("1KB"), Some(1024));
        assert_eq!(parse_bytes("1kb"), Some(1024));
        assert_eq!(parse_bytes("1K"), Some(1024));
        assert_eq!(parse_bytes("1MB"), Some(1024 * 1024));
        assert_eq!(parse_bytes("2 MB"), Some(2 * 1024 * 1024));
        assert_eq!(parse_bytes("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_bytes("1.5GB"), Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64));
    }

    #[test]
    fn parse_bytes_invalid() {
        assert_eq!(parse_bytes("abc"), None);
        assert_eq!(parse_bytes(""), None);
        assert_eq!(parse_bytes("1TB"), None); // unsupported suffix
    }

    // ---- human_bytes ----

    #[test]
    fn human_bytes_units() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(human_bytes(1024 * 1024 * 1024), "1.00 GB");
    }

    // ---- ScanFilter::excluded ----

    fn empty_filter() -> ScanFilter {
        ScanFilter {
            min_size: 0,
            include_globs: vec![],
            ignore_name_globs: vec![],
            ignore_exts: vec![],
            exclude_dirs: vec![],
            exclude_components: vec![],
        }
    }

    #[test]
    fn excluded_by_dir_prefix() {
        let mut f = empty_filter();
        f.exclude_dirs = vec![PathBuf::from("/Users/foo/secret")];
        assert!(f.excluded(Path::new("/Users/foo/secret/a.txt")));
        assert!(f.excluded(Path::new("/Users/foo/secret/sub/b.txt")));
        assert!(!f.excluded(Path::new("/Users/foo/public/a.txt")));
    }

    #[test]
    fn excluded_by_component_name() {
        let mut f = empty_filter();
        f.exclude_components = vec!["node_modules".into(), "target".into()];
        assert!(f.excluded(Path::new("/proj/node_modules/pkg/index.js")));
        assert!(f.excluded(Path::new("/proj/target/debug/exe")));
        assert!(!f.excluded(Path::new("/proj/src/main.rs")));
        // a file literally named "target" (no extension) is still excluded by component
        assert!(f.excluded(Path::new("/proj/target")));
    }

    #[test]
    fn excluded_neither() {
        let f = empty_filter();
        assert!(!f.excluded(Path::new("/anywhere/file.txt")));
    }

    // ---- ScanFilter::accepts ----

    fn meta_with_size(size: u64) -> fs::Metadata {
        // We can't easily fake Metadata; use a real temp file of the given size.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        if size > 0 {
            let f = fs::OpenOptions::new().write(true).open(tmp.path()).unwrap();
            f.set_len(size).unwrap();
        }
        fs::metadata(tmp.path()).unwrap()
    }

    #[test]
    fn accepts_min_size() {
        let mut f = empty_filter();
        f.min_size = 100;
        assert!(!f.accepts(Path::new("/x/a.txt"), &meta_with_size(50)));
        assert!(f.accepts(Path::new("/x/a.txt"), &meta_with_size(150)));
        assert!(f.accepts(Path::new("/x/a.txt"), &meta_with_size(100))); // boundary inclusive
    }

    #[test]
    fn accepts_ignore_name_glob() {
        let mut f = empty_filter();
        f.ignore_name_globs = vec![Pattern::new("thumb*").unwrap()];
        assert!(!f.accepts(Path::new("/x/thumb_1.jpg"), &meta_with_size(10)));
        assert!(f.accepts(Path::new("/x/photo.jpg"), &meta_with_size(10)));
    }

    #[test]
    fn accepts_ignore_ext() {
        let mut f = empty_filter();
        f.ignore_exts = vec!["log".into(), "tmp".into()];
        assert!(!f.accepts(Path::new("/x/debug.log"), &meta_with_size(10)));
        assert!(!f.accepts(Path::new("/x/cache.TMP"), &meta_with_size(10))); // case-insensitive
        assert!(f.accepts(Path::new("/x/data.json"), &meta_with_size(10)));
    }

    #[test]
    fn accepts_include_glob() {
        let mut f = empty_filter();
        f.include_globs = vec![Pattern::new("*.jpg").unwrap(), Pattern::new("*.png").unwrap()];
        assert!(f.accepts(Path::new("/x/photo.jpg"), &meta_with_size(10)));
        assert!(f.accepts(Path::new("/x/photo.png"), &meta_with_size(10)));
        assert!(!f.accepts(Path::new("/x/photo.gif"), &meta_with_size(10)));
    }

    // ---- scan (integration) ----

    #[test]
    fn scan_finds_duplicate_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // /a/x.txt and /b/x.txt have identical content -> duplicates
        // /a/unique.txt is unique -> should not appear
        // /c/sub/x.txt also identical -> third folder with a dup
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::create_dir_all(root.join("c/sub")).unwrap();

        let same = b"hello-world-duplicate";
        for dir in ["a", "b", "c/sub"] {
            let mut f = fs::File::create(root.join(dir).join("x.txt")).unwrap();
            f.write_all(same).unwrap();
        }
        let mut u = fs::File::create(root.join("a/unique.txt")).unwrap();
        u.write_all(b"unique-content-not-a-dup").unwrap();

        let filter = empty_filter();
        let folders = scan(root, &filter);

        // Three folders should appear: a, b, c/sub (each contains a dup of x.txt)
        let folder_names: Vec<String> = folders.iter().map(|f| f.folder.clone()).collect();
        assert_eq!(folders.len(), 3, "expected 3 folders, got {:?}", folder_names);

        // Each folder should list exactly the one duplicate file (x.txt),
        // NOT unique.txt (it was skipped by the size prefilter).
        for fv in &folders {
            assert_eq!(fv.files.len(), 1, "folder {} should have 1 file", fv.folder);
            assert!(fv.files[0].path.ends_with("x.txt"));
        }

        // No folder should be kept by default.
        assert!(folders.iter().all(|f| !f.keep));
    }

    #[test]
    fn scan_no_duplicates_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        let mut f1 = fs::File::create(root.join("a/one.txt")).unwrap();
        f1.write_all(b"content-one").unwrap();
        let mut f2 = fs::File::create(root.join("b/two.txt")).unwrap();
        f2.write_all(b"content-two-different").unwrap();

        let filter = empty_filter();
        let folders = scan(root, &filter);
        assert!(folders.is_empty(), "expected no duplicate folders, got {:?}", folders);
    }

    // ---- plan_move (integration) ----

    #[test]
    fn plan_move_excludes_kept_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let target = root.join("target");

        fs::create_dir_all(root.join("keep_me")).unwrap();
        fs::create_dir_all(root.join("move_me")).unwrap();
        let content = b"dup-content";
        let mut a = fs::File::create(root.join("keep_me/f.txt")).unwrap();
        a.write_all(content).unwrap();
        let mut b = fs::File::create(root.join("move_me/f.txt")).unwrap();
        b.write_all(content).unwrap();

        let filter = empty_filter();
        let mut folders = scan(root, &filter);
        // mark keep_me as kept
        for fv in folders.iter_mut() {
            if fv.folder.ends_with("keep_me") {
                fv.keep = true;
            }
        }

        let (plan, errors) = plan_move(&folders, root, &target);
        assert!(errors.is_empty(), "{:?}", errors);
        // only the move_me file should be in the plan
        assert_eq!(plan.len(), 1);
        assert!(plan[0].src.ends_with("move_me/f.txt"));
        assert!(plan[0].dest.ends_with("target/move_me/f.txt"));
    }

    // ---- prefs union-merge ----

    #[test]
    fn prefs_default_exclude_components_include_node_modules() {
        let p = Prefs::default_values();
        assert!(p.exclude_components.contains(&"node_modules".to_string()));
        assert!(p.exclude_components.contains(&"target".to_string()));
        assert!(p.exclude_components.contains(&".git".to_string()));
    }

    #[test]
    fn prefs_default_bind_and_open_browser() {
        let p = Prefs::default_values();
        assert_eq!(p.bind, "127.0.0.1:8787");
        assert!(p.open_browser);
    }
}