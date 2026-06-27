# Duplicate Finder (yafdt) — Project Status

**Last updated:** 2026-06-27
**State:** Shippable. All planned work is done; 29 tests pass under both default and `--features gui` builds.
**Binary name:** `dupe_finder`

---

## What it is

A CLI tool that scans a folder tree for duplicate files (by content hash), then serves a web UI (or an optional native egui GUI) where the user picks which **folders** to keep in place; every duplicate file in a non-kept folder gets moved to a target folder, preserving its full relative path.

- Self-contained Rust binary; opens a browser window on launch (default) or runs an egui window (`--gui`, requires the `gui` feature).
- Per-user preferences stored cross-platform via `dirs::config_dir()`.
- Web UI: axum + server-side rendered HTML + Bootstrap 5 + jQuery, all **vendored and embedded** (works offline).
- Never overwrites files — conflicts get `.1`/`.2`/... suffixes.
- Auto-ignores the target folder during scan (so previously-moved files aren't re-scanned).

---

## How to build & run

```bash
cargo build                              # web UI (default)
cargo build --features gui               # web UI + egui GUI
cargo run -- --root <dir> --target <dir>
cargo run --features gui -- --root <dir> --target <dir> --gui
cargo test                               # 29 tests
cargo test --features gui                # same tests, with gui feature
```

### CLI flags

```
dupe_finder --root <dir> --target <dir> [filters] [options]
```

- `--root` *(required)* — folder to scan
- `--target` *(required)* — where non-kept duplicates get moved (full relative path preserved)
- `--min-size` — `1`, `1KB`, `5MB`, `100B`
- `--include` *(repeatable)* — glob patterns to include, e.g. `--include '*.jpg'`
- `--ignore-name` *(repeatable)* — glob patterns to skip by filename, e.g. `--ignore-name 'thumb*'`
- `--ignore-ext` *(repeatable)* — extensions to skip, e.g. `--ignore-ext log`
- `--exclude-dir` *(repeatable)* — directory paths to skip (prefix match)
- `--exclude-component` *(repeatable)* — directory name fragments to skip (any path component), e.g. `node_modules`, `target`
- `--hash` — `md5` (default) | `xxhash` (fast, non-crypto) | `sha256` (paranoid)
- `--bind` — default `127.0.0.1:8787` (from prefs)
- `--no-browser` — don't auto-open the browser
- `--gui` — launch native egui GUI instead of web UI (needs `gui` feature)
- `--dry-run` — print what would move (src -> dest, total bytes) and exit, no server
- `--save-prefs` — persist current settings to user prefs file and exit
- `--no-prefs` — ignore the prefs file for this run

### Preferences file

Location (`dirs::config_dir()`):
- macOS: `~/Library/Application Support/dupe_finder/prefs.json`
- Linux: `~/.config/dupe_finder/prefs.json`
- Windows: `%APPDATA%\dupe_finder\prefs.json`

Stored keys (never `root`/`target` — those are job-specific):
- `exclude_components` — defaults: `node_modules, target, .git, .svn, .hg, vendor, __pycache__, .venv, venv, .next, .nuxt, .cache, .Trash`
- `ignore_names` — defaults: `.DS_Store, ._*, Thumbs.db, desktop.ini`
- `exclude_dirs`, `min_size`, `ignore_exts`, `include`
- `bind`, `open_browser`

Merge behavior: CLI flags + prefs = **union for lists** (so `node_modules` always stays excluded), **CLI wins for scalars**.

---

## Architecture

The codebase is split into modules under `src/`:

| File | Responsibility | ~Lines |
|---|---|---|
| `main.rs` | Thin entry: CLI parse, filter build, dispatch to web/gui/dry-run | 290 |
| `cli.rs` | `Cli` struct, `parse_bytes`, `human_bytes` | 105 |
| `prefs.rs` | `Prefs` (per-user config) load/save | 115 |
| `model.rs` | `FileEntry`, `Folder`, `Md5`, `AppState`, `SharedState` | 85 |
| `scan.rs` | `HashAlgo`, `ScanFilter`, `hash_file`, `scan`, `scan_with_progress` | 240 |
| `move_files.rs` | `MovePlanItem`, `unique_dest`, `plan_move`, `execute_move`, `move_non_kept` | 145 |
| `assets.rs` | `rust-embed` `StaticAssets`, `index_handler`, `vendor_handler` | 40 |
| `web.rs` | axum handlers + `build_router` | 215 |
| `gui.rs` | egui app (`gui` feature-gated) | 130 |
| `tests.rs` | all 29 tests | 470 |
| `static/index.html` | web UI (Bootstrap 5 + jQuery, vendored) | 240 |
| `static/vendor/*` | bootstrap.min.css, jquery.min.js, bootstrap.bundle.min.js | (embedded) |

**Key types:**

```rust
type Md5 = String;

struct FileEntry { path: String, size: u64 }            // display-only, no per-file keep
struct Folder { folder: String, keep: bool, files: Vec<FileEntry>, total_size: u64 }

struct AppState {                                        // shared via Arc<std::sync::Mutex<AppState>>
    root, target: PathBuf,
    folders: Vec<Folder>,
    scanning: bool, scan_error: Option<String>,
    walked, hashed: Arc<AtomicUsize>,                   // live scan counters
}

enum HashAlgo { Md5, Xxhash, Sha256 }
struct ScanFilter { min_size, include_globs, ignore_name_globs, ignore_exts,
                    exclude_dirs, exclude_components, hash_algo }
struct MovePlanItem { src: String, dest: String, size: u64 }
```

**Data model (folder-only):** Scan -> list of folders that contain duplicates -> user marks whole folders to keep in place -> Move sends files in non-kept folders to `target/<rel path>`. No per-file keep, no duplicate groups, no keep-strategy. Move is blocked until >=1 folder is kept.

**Scan flow:** Two-pass for performance. Pass 1 walks + buckets every accepted file by size (cheap, no I/O reads). Pass 2 hashes only files whose size bucket has >1 entry (unique-size files skip hashing entirely). Hashes stream via `BufReader` (O(1) memory). On the web path the server starts immediately with `scanning=true` and empty folders; a background thread runs the scan, deposits folders, and flips `scanning=false`. The browser polls `/api/scan_progress` for a live counter.

**Move flow:** `plan_move` (pure) builds the src->dest list, applying `unique_dest` (never overwrites — appends `.1`/`.2`/... to the full filename). `execute_move` re-resolves at execution time (a prior item in the same batch may have created the file), creates parent dirs, and does `fs::rename` with a cross-volume `copy+delete` fallback on `EXDEV`. Per-file errors are collected, not fatal.

## API routes

- `GET  /` — SSR HTML
- `GET  /vendor/:file` — vendored static asset
- `GET  /api/state` — `{ root, target, folders, kept_folders, total_files, total_size, scanning, scan_error }`
- `GET  /api/scan_progress` — `{ scanning, scan_error, walked, hashed }`
- `POST /api/mark_folder` — `{ folder, keep }` (cascades to subfolders: `starts_with` match)
- `POST /api/mark_all` — `{ keep }` (keep all / keep none)
- `POST /api/preview` — builds the move plan without executing (same kept-folder guard)
- `POST /api/move` — moves files in non-kept folders; returns `{ ok, moved, kept_folders, errors[] }`

---

## Buckets: done vs open

### DONE — Core features
- [x] CLI (clap) with `--root`/`--target` + filters (`--include`, `--ignore-name`, `--ignore-ext`, `--exclude-dir`, `--exclude-component`, `--min-size`)
- [x] Per-user prefs via `dirs` (mac/linux/win), `--save-prefs`/`--no-prefs`, union merge
- [x] Scan by content hash -> folders containing duplicates (folder-only model)
- [x] Keep folders in place, move the rest to `target/<full rel path>`
- [x] Move blocked until >=1 folder kept (UI + handler guard)
- [x] Cross-volume fallback (copy+delete on EXDEV)
- [x] Per-file errors collected (batch doesn't abort); surfaced as an inline `<ul>` panel in the UI
- [x] Web UI: axum + SSR HTML + Bootstrap/jQuery, folder list + keep checkboxes
- [x] Auto-open browser on launch (default)
- [x] Native egui GUI behind `gui` feature, `--gui` flag
- [x] Self-contained binary (HTML + Bootstrap + jQuery embedded via `rust-embed`, works offline)

### DONE — Roadmap (all 6 items)
- [x] **#1 Hashing performance** — size prefilter (bucket by size, only hash groups >1) + streaming `BufReader` hash (O(1) memory)
- [x] **#2 Dry-run / preview** — `--dry-run` CLI flag prints the plan; `POST /api/preview` returns the plan; web UI "Preview move" button shows a src/dest/size table
- [x] **#3 Vendor assets** — bootstrap/jQuery downloaded into `src/static/vendor/`, served via `/vendor/:file`, works offline
- [x] **#4 Tests** — 29 tests (unit + integration); `tempfile` dev-dependency
- [x] **#5 Live scan progress** — scan runs on a background thread; `GET /api/scan_progress`; web UI polls and shows a spinner + live walked/hashed counter
- [x] **#6 Keep-all-subfolders** — `mark_folder` cascades recursively (`starts_with`); web badge "keep (+subfolders)"; egui "Keep this folder + subfolders"

### DONE — Safety hardening
- [x] **Auto-ignore target folder** during scan (added to `exclude_dirs` so moved files aren't re-scanned)
- [x] **Never overwrite** — `unique_dest` appends `.1`/`.2`/... to the full filename; applied in both `plan_move` (preview shows real dest) and `execute_move` (re-resolves at exec time)
- [x] **Per-file error UI** — move errors render as an inline `<ul>` panel, not a blocking alert

### DONE — Maybe-later / polish
- [x] **Configurable hash** — `--hash md5|xxhash|sha256`; `HashAlgo` enum; all three stream via `BufReader`
- [x] **Deleted stale `config.json`** — superseded by per-user prefs; gitignored
- [x] **OS-noise auto-ignore** — `.DS_Store`, `._*`, `Thumbs.db`, `desktop.ini` in default `ignore_names`

### OPEN — Nothing planned
The roadmap, safety hardening, and polish items are all complete. There is no outstanding work.

---

## Tests (29 total, all passing)

Run: `cargo test` and `cargo test --features gui`.

| Area | Tests |
|---|---|
| `parse_bytes` | plain number, B suffix, KB/MB/GB (incl. decimals, case, spaces), invalid, unsupported suffix |
| `human_bytes` | unit boundaries (B/KB/MB/GB) |
| `ScanFilter::excluded` | exclude-dir prefix, exclude-component name (`node_modules`/`target`), neither |
| `ScanFilter::accepts` | min-size (boundary inclusive), ignore-name glob, ignore-ext (case-insensitive), include glob |
| `scan` (integration) | finds 3 duplicate folders + skips unique-size file; no dups -> empty |
| `plan_move` | excludes kept folders; uses `unique_dest` when target exists (preview reflects `.1`) |
| `move` (e2e) | `e2e_move_preserves_paths_and_no_overwrite` — builds test_data-style tree, two moves, asserts target tree + `.1` suffix + kept folder untouched |
| `move` (errors) | per-file error collection, batch continues (real file moves, ghost file errors) |
| `unique_dest` | no-conflict, with-extension (`.1`/`.2`), no-extension |
| `target auto-ignore` | a file already in target matching dup content does not appear as a third copy |
| `HashAlgo` | parse valid (md5/xxhash/sha256, case+whitespace), invalid (crc32/empty/sha1), `as_str` |
| cross-algo | all three algos find the same duplicates |
| `Prefs` defaults | exclude_components (node_modules/target/.git), ignore_names (.DS_Store/._*/Thumbs.db/desktop.ini), bind, open_browser |

---

## Ideas / Future directions (not started, low priority)

These are stretch ideas if the tool ever needs more. None are required for the tool to be useful.

- **Big-tree benchmark** — the size prefilter was verified for correctness but never benchmarked on a real large folder (e.g. a full photo library) to quantify the speedup. Would confirm the "10-50x faster" claim.
- **xxhash as the default** — md5 is the current default for familiarity; xxhash is ~5x faster. Could flip the default if perf matters more than crypto-comfort.
- **SSE/WebSocket scan progress** — currently polled at 700ms; Server-Sent Events would be marginally slicker but adds complexity for little gain.
- **Rescan button in the UI** — currently you restart the binary to re-scan. A `/api/rescan` endpoint + button would let the user re-scan without leaving the browser.
- **Per-folder preview** — preview currently shows the whole move plan; a per-folder "what would move from this folder" view could help with large trees.
- **Undo** — moves are irreversible by design (they're moves, not deletes, so files are recoverable from the target). A real undo would require tracking the src->dest mapping. Probably not worth it.
- **Configurable keep strategy** — currently the user picks folders manually. A "keep newest/oldest/largest per group" bulk action was considered and **rejected** (the folder-only model replaces it). Could be revisited if a user has hundreds of folders, but the recursive keep-subfolders cascade already handles the common case.
- **CI** — no GitHub Actions workflow. A simple `cargo test --all-features` on push would prevent regressions.
- **Release binaries** — no cross-compilation/release build setup. `cargo build --release --features gui` produces a working binary; cross-comp to linux/windows from macOS would need cross-toolchains.

---

## Design decisions (so they're not relitigated)

- **Folder-only model, not per-file.** Users keep whole folders in place; everything else moves. This replaced an earlier per-file/per-group model that was too tedious for real-world use (hundreds of groups). Do not re-add per-file keep checkboxes or per-group "keep only" radios.
- **Keep-strategy (newest/oldest/largest) was rejected.** The folder-only model + recursive keep-subfolders cascade handles the same need more intuitively.
- **`config.json` in the repo root was removed.** Superseded by per-user prefs via `dirs`. Do not reintroduce a repo-local config file.
- **md5 is the default hash.** It's only used for equality comparison, not cryptographic integrity, so collisions aren't a concern. xxhash (faster) and sha256 (paranoid) are available via `--hash`.
- **`std::sync::Mutex`, not `tokio::sync::Mutex`.** The axum handlers don't await while holding the lock, and the egui render callback runs on the tokio runtime thread where `tokio::sync::Mutex::blocking_lock` would panic. A single `std::sync::Mutex` works for both.
- **axum 0.7** (not 0.8). Path params use `:file` syntax (axum 0.7); 0.8+ uses `{file}`. If upgrading axum, update the `/vendor/:file` route.
- **Browser is the default UI; egui is opt-in** via `--gui` + the `gui` feature. The web UI is the primary surface.

---

## Commit history

```
31bf8b7 configurable hash (--hash md5|xxhash|sha256); ignore OS noise; drop stale config.json
5ac00c6 split main.rs into modules
9b0258a safety: auto-ignore target, never overwrite, per-file error UI; +7 tests
8ef4c1a live scan progress (#5); recursive keep-subfolders cascade (#6)
7d317c6 vendor bootstrap/jquery for offline use; add test suite (#3, #4)
c6e08b5 perf: size prefilter + streaming hash; safety: dry-run/preview
ee04364 switch to folder-only model: keep folders, move the rest
2db0f48 add roadmap and todo docs
c2fa4a4 add native gui
31269ee use html and jquery server side
0a0bbb4 first commit
0edc4cc Initial commit
```

The earliest commits reflect the project's evolution: it started as an egui-only native app, gained a web UI (replacing the original plan), then went through a folder-only redesign, performance work, safety hardening, and modularization. The commit messages are descriptive enough to reconstruct the rationale for any change.

---

## How to revive this project

1. `git clone` and `cargo build --features gui` — confirms the toolchain works.
2. `cargo test` — confirms 29 tests pass.
3. Read this file (`PROJECT_STATUS.md`) for the full context; it's self-contained.
4. `./target/debug/dupe_finder --root test_data --target /tmp/out --no-browser` then open `http://127.0.0.1:8787` to see the UI.
5. To extend: pick an item from "Ideas / Future directions" above. The module layout (`scan.rs`, `move_files.rs`, `web.rs`, etc.) makes the seams clear. Tests live in `src/tests.rs` and cover the pure logic, so refactors are low-risk.