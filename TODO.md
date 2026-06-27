# TODO — Duplicate Finder

## Next up (recommended pair)

### [x] 1. Hashing performance  ✅ DONE (commit pending)
- [x] Add a size-bucketing pass before hashing: collect `(path, size)` for all accepted files, group by size, only hash files whose size group has >1 entry.
- [x] Replace `md5sum` (full `read_to_end`) with a streaming `BufReader` + chunked `Read` into `Md5::Context` (via `io::copy` into the `Write` impl).
- [x] Verify: unique-size files never get hashed; memory stays flat on large files. (test_data: walked 7, hashed 6 — 1 unique-size file skipped)
- [ ] Bench against old impl on a larger real folder. *(deferred — correctness verified, big-tree bench not yet run)*

### [x] 2. Dry-run / preview before move  ✅ DONE (commit pending)
- [x] Add `--dry-run` CLI flag.
- [x] Refactor `move_non_kept` to split "plan" (`plan_move`, pure) from "execute" (`execute_move`, does `fs::rename`).
- [x] In dry-run, return the plan without executing; print src→dest + total bytes to terminal.
- [x] Add a "Preview" button in `index.html` that calls `/api/preview` and shows the list in a panel (src/dest/size table).
- [x] Make sure Move still requires ≥1 kept folder in dry-run too. (`/api/preview` has the same guard as `/api/move`.)

## After that

### [x] 3. Vendor Bootstrap/jQuery for offline self-contained binary  ✅ DONE (commit pending)
- [x] Download `bootstrap.min.css`, `jquery.min.js`, `bootstrap.bundle.min.js` into `src/static/vendor/`.
- [x] Update `index.html` `<link>`/`<script>` to local paths (`/vendor/...`).
- [x] Add axum route to serve `/vendor/*` from `rust-embed` (`/vendor/:file` — axum 0.7 syntax).
- [x] Confirm binary works with network off. (assets embedded via rust-embed; served 200 with correct content-type)

### [x] 4. Tests  ✅ DONE (commit pending)
- [x] Add `tempfile` as a dev-dependency.
- [x] Unit tests: `parse_bytes` (KB/MB/GB/no-suffix/invalid).
- [x] Unit tests: `ScanFilter::excluded` (exclude-dir prefix, exclude-component match, neither).
- [x] Unit tests: `ScanFilter::accepts` (min-size, ignore-name glob, ignore-ext, include glob).
- [x] Integration test: build a temp tree with known duplicates, run `scan`, assert folder list.
- [x] Unit tests: `human_bytes`, prefs defaults.
- [x] Run via `cargo test`. (17 tests, all pass; also pass with `--features gui`.)

### [x] 5. Live scan progress  ✅ DONE (commit pending)
- [x] Move `scan()` off the main thread (spawn + atomics). Web path now starts the server immediately with `scanning=true` and empty folders; a background thread runs `scan_with_progress`, deposits folders, and flips `scanning=false`.
- [x] Add `GET /api/scan_progress` returning `{ scanning, scan_error, walked, hashed }`.
- [x] In `index.html`, poll progress while `scanning=true`; show a spinner + live `walked/hashed` counter before the folder list loads.
- [x] `--gui` and `--dry-run` paths still scan synchronously (they need the result before launching / printing).
- [x] egui GUI shows the same counters via the shared atomics.

### [x] 6. Keep-all-subfolders helper  ✅ DONE (commit pending)
- [x] `mark_folder` now cascades recursively: matching `f_canon == target || f_canon.starts_with(target)` — keeping `/photos/2024` also keeps `/photos/2024/jan`, etc.
- [x] Web UI keep badge now reads "keep (+subfolders)".
- [x] egui GUI checkbox relabeled "Keep this folder + subfolders" with the same cascade.

## Maybe later

- [x] Per-file error reporting surfaced in the UI (move errors now render as an inline `<ul>` panel instead of a blocking alert). ✅ DONE
- [x] Automatically ignore the target folder during scan (added to `exclude_dirs` so moved-into-target files are never re-scanned). ✅ DONE
- [x] Never overwrite files anywhere — `unique_dest` appends `.1`/`.2`/... to the full filename when the destination exists (e.g. `Cargo.toml` → `Cargo.toml.1`). Applied in both `plan_move` (preview shows the real dest) and `execute_move`. ✅ DONE
- [x] Configurable hash algorithm (`--hash md5|xxhash|sha256`) behind a flag. ✅ DONE
- [x] Delete the stale `config.json` in the repo root (superseded by per-user prefs). ✅ DONE
- [x] `.DS_Store` auto-ignore (macOS noise) — added to default `ignore_names` (also `._*`, `Thumbs.db`, `desktop.ini`). ✅ DONE