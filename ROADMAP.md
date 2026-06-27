# Duplicate Finder — Project State & Roadmap

Last updated: 2026-06-27 (all roadmap + safety + maybe-later items done)

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

## 1. Hashing performance — DONE ✅
`md5sum` now streams via `BufReader` + `io::copy` into `md5::Context` (O(1) memory). `scan` is two-pass: walk + bucket by size, then hash only size groups with >1 file. Unique-size files skip hashing entirely.

## 2. Dry-run / preview before move — DONE ✅
`--dry-run` CLI flag prints the full move plan (src → dest, total bytes) and exits without touching disk. `move_non_kept` split into `plan_move` (pure) + `execute_move`. New `POST /api/preview` returns the plan (same kept-folder guard as move). Web UI has a "Preview move" button showing a src/dest/size table.

## 3. Self-contained binary (vendored assets) — DONE ✅
Bootstrap/jQuery vendored into `src/static/vendor/` and embedded via `rust-embed`. New `GET /vendor/:file` route serves them. `index.html` references local `/vendor/*` paths. Binary works fully offline (~150KB added).

## 4. Tests — DONE ✅
17 inline unit + integration tests in `src/main.rs` (`#[cfg(test)] mod tests`): `parse_bytes`, `human_bytes`, `ScanFilter::excluded`/`accepts`, `scan` (with `tempfile` fixtures), `plan_move`, prefs defaults. Pass with and without `--features gui`.

## 5. Live scan progress — DONE ✅
Web path now starts the server immediately with `scanning=true` and empty folders; a background thread runs `scan_with_progress` (taking `AtomicUsize` counters), deposits the folders, and flips `scanning=false`. New `GET /api/scan_progress` returns `{ scanning, scan_error, walked, hashed }`. `index.html` polls it every 700ms and shows a spinner + live counter until the scan finishes, then fetches the folder list. `--gui`/`--dry-run` still scan synchronously (they need the result before launching/printing).

## 6. Keep-all-subfolders helper — DONE ✅
`mark_folder` cascades recursively: `f_canon == target || f_canon.starts_with(target)` — keeping `/photos/2024` also keeps `/photos/2024/jan`. Web UI keep badge reads "keep (+subfolders)"; egui checkbox relabeled "Keep this folder + subfolders" with the same cascade.

---

# Roadmap / Next steps (priority order)

## Done
1. Hashing performance (size prefilter + streaming hash) — ✅
2. Dry-run / preview before move — ✅
3. Self-contained binary (vendored assets) — ✅
4. Tests — ✅
5. Live scan progress — ✅
6. Keep-all-subfolders helper — ✅

---

# Deferred / rejected ideas

- **Keep-strategy (newest/oldest/largest per group)** — rejected. The folder-only model replaces this; users keep whole folders, not individual files. No per-group logic.
- **Per-file keep checkboxes** — removed. Folder-only is the model.
- **Duplicate groups in the UI** — removed. Folders are the unit.
- **`config.json` in the repo root** — superseded by per-user prefs via `dirs`. File is now unused; safe to delete.

---

# Safety hardening (post-roadmap)

- **Auto-ignore target folder**: the `--target` is added to `exclude_dirs` automatically, so files already moved there on a previous run are never re-scanned as duplicate sources.
- **Never overwrite**: `unique_dest(dest)` appends `.1`, `.2`, ... to the full filename when the destination already exists (e.g. `Cargo.toml` → `Cargo.toml.1`). Applied in both `plan_move` (preview shows the real dest) and `execute_move` (re-resolves at execution time in case a prior item in the same batch created the file).
- **Per-file error UI**: `/api/move` and `/api/preview` return per-file `errors[]`; the web UI renders them as an inline `<ul>` panel (not a blocking alert) so the batch continues and every failure is visible.
- **Tests**: 29 total — including an e2e test (`e2e_move_preserves_paths_and_no_overwrite`) that builds a test_data-style tree, does two moves, and asserts both the target tree structure and the `.1` non-overwrite suffix; plus targeted tests for `unique_dest`, target auto-ignore, per-file error collection, `HashAlgo` parsing, cross-algo duplicate detection, and `.DS_Store` default ignore.

## Configurable hash

`--hash md5` (default) | `xxhash` (fast, non-crypto) | `sha256` (paranoid). All stream via `BufReader`; xxhash uses a manual chunked `Hasher::write` loop (it doesn't impl `io::Write`). The digest is only used to compare file equality.

## OS-noise auto-ignore

`Prefs::default_ignore_names()` ships `.DS_Store`, `._*`, `Thumbs.db`, `desktop.ini` so macOS/Windows metadata never appears as duplicate candidates.