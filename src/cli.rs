use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "dupe_finder", about = "Find duplicate files and move them via a web UI")]
pub struct Cli {
    /// Root folder to search for duplicates.
    #[arg(long)]
    pub root: String,

    /// Target folder where non-kept duplicates get moved. Preserves full relative path.
    #[arg(long)]
    pub target: String,

    /// Minimum file size (e.g. 1, 1KB, 5MB, 100B). Default: no minimum.
    #[arg(long)]
    pub min_size: Option<String>,

    /// Include only files matching these glob patterns (e.g. '*.jpg'). Default: all.
    #[arg(long = "include", value_name = "GLOB")]
    pub include: Vec<String>,

    /// Ignore files whose names match any of these glob patterns (e.g. 'thumb*', '*.tmp').
    #[arg(long = "ignore-name", value_name = "GLOB")]
    pub ignore_names: Vec<String>,

    /// Ignore files with any of these extensions (e.g. 'log', 'tmp'). Leading dot optional.
    #[arg(long = "ignore-ext", value_name = "EXT")]
    pub ignore_exts: Vec<String>,

    /// Directory paths to skip (matched by prefix). Can be repeated.
    #[arg(long = "exclude-dir", value_name = "DIR")]
    pub exclude_dirs: Vec<String>,

    /// Directory name fragments to always skip (matched against any path
    /// component, e.g. 'node_modules', 'target'). Defaults come from prefs.
    #[arg(long = "exclude-component", value_name = "NAME")]
    pub exclude_components: Vec<String>,

    /// Address to serve the web UI on. Defaults from prefs (127.0.0.1:8787).
    #[arg(long)]
    pub bind: Option<String>,

    /// Do not open a browser automatically.
    #[arg(long)]
    pub no_browser: bool,

    /// Launch the native egui GUI instead of the web UI (requires the `gui` feature).
    #[arg(long)]
    pub gui: bool,

    /// Save the current flags (exclude-components, exclude-dirs, min-size, ignore-*,
    /// include, bind, open-browser) to the user prefs file and exit.
    #[arg(long)]
    pub save_prefs: bool,

    /// Ignore the user prefs file entirely for this run.
    #[arg(long)]
    pub no_prefs: bool,

    /// Don't move anything. Print what *would* be moved (src -> dest, total
    /// bytes) to the terminal and exit. Useful as a pre-flight check.
    #[arg(long)]
    pub dry_run: bool,

    /// Hash algorithm: "md5" (default), "xxhash" (fast, non-crypto), or "sha256"
    /// (paranoid). All produce a hex digest used only to compare equality.
    #[arg(long, default_value = "md5")]
    pub hash: String,
}

/// Parse a human byte size: "1", "1KB", "5MB", "100B", "1.5GB". Returns None
/// on unparseable input or unsupported suffixes.
pub fn parse_bytes(s: &str) -> Option<u64> {
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
pub fn human_bytes(b: u64) -> String {
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
