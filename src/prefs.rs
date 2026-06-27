use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::PathBuf,
};

/// Per-user preferences, stored cross-platform via `dirs::config_dir()`:
///   - macOS:   ~/Library/Application Support/dupe_finder/prefs.json
///   - Linux:   ~/.config/dupe_finder/prefs.json
///   - Windows: %APPDATA%\dupe_finder\prefs.json
///
/// Never stores `root`/`target` — those are job-specific CLI args.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prefs {
    /// Directory name fragments to always exclude (matched against any path
    /// component). Sensible defaults: node_modules, target, .git, ...
    #[serde(default = "Prefs::default_exclude_components")]
    pub exclude_components: Vec<String>,
    /// Absolute directory paths to always skip (prefix match).
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    /// Default min file size as a human string ("1MB", ...). Empty = no minimum.
    #[serde(default)]
    pub min_size: Option<String>,
    /// Always ignore files whose names match these globs.
    #[serde(default = "Prefs::default_ignore_names")]
    pub ignore_names: Vec<String>,
    /// Always ignore files with these extensions.
    #[serde(default)]
    pub ignore_exts: Vec<String>,
    /// Default include globs (empty = all files).
    #[serde(default)]
    pub include: Vec<String>,
    /// Bind address for the web UI.
    #[serde(default = "Prefs::default_bind")]
    pub bind: String,
    /// Open the browser automatically.
    #[serde(default = "Prefs::default_open_browser")]
    pub open_browser: bool,
}

impl Prefs {
    pub fn default_exclude_components() -> Vec<String> {
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
    pub fn default_bind() -> String { "127.0.0.1:8787".into() }
    pub fn default_open_browser() -> bool { true }

    /// macOS metadata noise, desktop.ini thumbnails, Thumbs.db caches — ignored
    /// by default so they never appear as duplicate candidates.
    pub fn default_ignore_names() -> Vec<String> {
        vec![
            ".DS_Store".into(),
            "._*".into(),      // macOS AppleDouble resource-fork sidecar files
            "Thumbs.db".into(),
            "desktop.ini".into(),
        ]
    }

    pub fn config_path() -> Option<PathBuf> {
        let base = dirs::config_dir()?;
        Some(base.join("dupe_finder").join("prefs.json"))
    }

    pub fn load() -> Prefs {
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

    pub fn default_values() -> Prefs {
        Prefs {
            exclude_components: Self::default_exclude_components(),
            exclude_dirs: Vec::new(),
            min_size: None,
            ignore_names: Self::default_ignore_names(),
            ignore_exts: Vec::new(),
            include: Vec::new(),
            bind: Self::default_bind(),
            open_browser: Self::default_open_browser(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
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
