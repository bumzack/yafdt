# Duplicate Finder — Project State & Roadmap

Last updated: 2026-06-27

## What this app is

A CLI tool that scans a folder tree for duplicate files (by md5 content hash), then serves a web UI (or an optional native egui GUI) where the user picks which **folders** to keep in place; every duplicate file in a non-kept folder gets moved to a target folder, preserving its full relative path.

- Self-contained Rust binary; opens a browser window on launch (default) or runs an egui window (`--gui`, requires the `gui` feature).
- Per-user preferences stored cross-platform via `dirs::config_dir()`.
- Web UI: axum + server-side rendered HTML + Bootstrap 5 + jQuery (CDN).

## CLI

```
dupe_finder --root <dir> --target <dir> [filters] [options]
```

Flags:
- `--root` *(required)* — folder to scan
- `--target` *(required)* — where non-kept duplicates get moved (full relative path preserved)
- `--min-size` — `1`, `1KB`, `5MB`, `100B`
- `--include` *(repeatable)* — glob patterns to include, e.g. `--include '*.jpg'`
- `--ignore-name` *(repeatable)* — glob patterns to skip by filename, e.g. `--ignore-name 'thumb*'`
- `--ignore-ext` *(repeatable)* — extensions to skip, e.g. `--ignore-ext log`
- `--exclude-dir` *(repeatable)* — directory paths to skip (prefix match)
- `--exclude-component` *(repeatable)* — directory name fragments to skip (any path component), e.g. `node_modules`, `target`
- `--bind` — default `127.0.0.1:8787` (from prefs)
- `--no-browser` — don't auto-open the browser
- `--gui` — launch native egui GUI instead of web UI (needs `gui` feature)
- `--save-prefs` — persist current settings to user prefs file and exit
- `--no-prefs` — ignore the prefs file for this run

## Preferences file

Location (`dirs::config_dir()`):
- macOS: `~/Library/Application Support/dupe_finder/prefs.json`
- Linux: `~/.config/dupe_finder/prefs.json`
- Windows: `%APPDATA%\dupe_finder\prefs.json`

Stored keys (never `root`/`target` — those are job-specific):
- `exclude_components` — defaults: `node_modules, target, .git, .svn, .hg, vendor, __pycache__, .venv, venv, .next, .nuxt, .cache, .Trash`
- `exclude_dirs`, `min_size`, `ignore_names`, `ignore_exts`, `include`
- `bind`, `open_browser`

Merge behavior: CLI flags + prefs = **union for lists** (so `node_modules` always stays excluded), **CLI wins for scalars**.

## Data model (folder-only)

```
Folder {
  folder: String,      // absolute parent path
  keep: bool,          // user marks whole folders to keep in place
  files: Vec<FileEntry>,  // display only: path + size
  total_size: u64,
}
FileEntry { path: String, size: u64 }
```

No per-file keep, no duplicate groups, no keep-strategy. Scan → list of folders containing duplicates → check folders to keep → Move sends files in non-kept folders to `target/<rel path>`.

Safety: Move is blocked until at least one folder is checked.

## API routes

- `GET  /` — SSR HTML
- `GET  /api/state` — `{ root, target, folders, kept_folders, total_files, total_size }`
- `POST /api/mark_folder` — `{ folder, keep }`
- `POST /api/mark_all` — `{ keep }` (keep all / keep none)
- `POST /api/move` — moves files in non-kept folders; returns `{ ok, moved, kept_folders, errors[] }` (per-file errors, doesn't abort batch)

## Architecture

- `src/main.rs` — everything: CLI, prefs, scan, state, handlers, main, egui module
- `src/static/index.html` — embedded via `rust-embed`
- `Cargo.toml` — `gui` feature gates `eframe`/`egui`

State shared via `Arc<std::sync::Mutex<AppState>>` (works for both axum handlers and egui render callback).

## Build

```
cargo build                              # web UI (default)
cargo build --features gui               # web UI + egui GUI
cargo run -- --root <dir> --target <dir>
cargo run --features gui -- --root <dir> --target <dir> --gui
```

---

# Roadmap / Next steps (priority order)

## 1. Hashing performance — HIGHEST IMPACT
Current `md5sum` reads the whole file into RAM (`read_to_end` into a `Vec`) — slow + memory-heavy on big media.

Two cheap wins:
- **Size pre-filter**: bucket files by size *first*; only hash groups with >1 file of the same size. Unique-size files skip hashing entirely. Huge win on mixed trees.
- **Stream the hash**: `BufReader` + chunked `Read` into `Md5` instead of slurping the whole file. O(1) memory.

Expected: 10–50x faster on photo libraries.

## 2. Dry-run / preview before move
`--dry-run` flag + "Preview" button: lists exactly what would move (source → dest, total bytes) without touching disk. Move is irreversible, so preview is the single biggest safety win.

Implementation: `move_non_kept` already builds (src, dest) pairs — just skip `fs::rename` when dry-run is on and return the list.

## 3. Self-contained binary (vendored assets)
Bootstrap/jQuery currently load from CDN → breaks offline. Vendor `bootstrap.min.css`, `jquery.min.js`, `bootstrap.bundle.min.js` into `src/static/` (already served via `rust-embed`) and point HTML at local paths. ~150KB added; works fully offline.

## 4. Tests
There are none. Pure-ish functions to unit-test with `tempfile` fixtures:
- `parse_bytes`
- `ScanFilter::excluded` / `accepts`
- size-bucketing logic (from #1)
- prefs union-merge

Insurance against #1–#3 regressing the folder-only model.

## 5. Live scan progress
Scan is synchronous and blocks `main`; page only loads after it finishes. For big roots: spawn scan on a thread, stream file count via `/api/scan_progress` (polled or SSE) so the browser shows a live counter. The old egui version did this with `crossbeam-channel`; easy to restore.

## 6. Keep-all-subfolders helper
A folder checkbox keeps that exact folder. If a user wants "keep everything under `/photos/2024`," they currently check every subfolder. Add `recursive: true` to `mark_folder` matching `parent.starts_with(folder)` — one line in handler, one checkbox in UI.

---

# Deferred / rejected ideas

- **Keep-strategy (newest/oldest/largest per group)** — rejected. The folder-only model replaces this; users keep whole folders, not individual files. No per-group logic.
- **Per-file keep checkboxes** — removed. Folder-only is the model.
- **Duplicate groups in the UI** — removed. Folders are the unit.
- **`config.json` in the repo root** — superseded by per-user prefs via `dirs`. File is now unused; safe to delete.