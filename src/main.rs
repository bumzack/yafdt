mod assets;
mod cli;
#[cfg(feature = "gui")]
mod gui;
mod model;
mod move_files;
mod prefs;
mod scan;
#[cfg(test)]
mod tests;
mod web;

use clap::Parser;
use cli::{human_bytes, parse_bytes, Cli};
use glob::Pattern;
use model::{AppState, SharedState};
use prefs::Prefs;
use scan::{scan, scan_with_progress, ScanFilter};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

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

    let mut exclude_dirs: Vec<PathBuf> = cli
        .exclude_dirs
        .iter()
        .chain(prefs.exclude_dirs.iter())
        .map(|s| fs::canonicalize(s).unwrap_or_else(|_| PathBuf::from(s)))
        .collect();

    // Always exclude the target folder (where duplicates get moved to) so that
    // files moved there on a previous run are never re-scanned as new sources.
    let target_canon = fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
    if !exclude_dirs.contains(&target_canon) {
        exclude_dirs.push(target_canon);
    }

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
        let mut state = AppState::new_for_scan(root, target, false);
        state.folders = folders;
        let shared: SharedState = Arc::new(Mutex::new(state));
        gui::run_gui(shared);
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
        let (plan, errors) = move_files::plan_move(&folders, &root, &target);
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
    let state = AppState::new_for_scan(root.clone(), target.clone(), true);
    let walked = state.walked.clone();
    let hashed = state.hashed.clone();
    let shared: SharedState = Arc::new(Mutex::new(state));

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

    let app = web::build_router(shared);

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
