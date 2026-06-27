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

### [ ] 3. Vendor Bootstrap/jQuery for offline self-contained binary
- [ ] Download `bootstrap.min.css`, `jquery.min.js`, `bootstrap.bundle.min.js` into `src/static/vendor/`.
- [ ] Update `index.html` `<link>`/`<script>` to local paths (`/vendor/...`).
- [ ] Add axum routes to serve `/vendor/*` from `rust-embed` (or reuse `serve_asset`).
- [ ] Confirm binary works with network off.

### [ ] 4. Tests
- [ ] Add `tempfile` as a dev-dependency.
- [ ] Unit tests: `parse_bytes` (KB/MB/GB/no-suffix/invalid).
- [ ] Unit tests: `ScanFilter::excluded` (exclude-dir prefix, exclude-component match, neither).
- [ ] Unit tests: `ScanFilter::accepts` (min-size, ignore-name glob, ignore-ext, include glob).
- [ ] Integration test: build a temp tree with known duplicates, run `scan`, assert folder list.
- [ ] Integration test: prefs union-merge (CLI + defaults = deduped union).
- [ ] Run via `cargo test`.

### [ ] 5. Live scan progress
- [ ] Move `scan()` off the main thread (spawn + channel, like the old egui version).
- [ ] Add `GET /api/scan_progress` returning `{ scanned, done }` (polled) or SSE stream.
- [ ] In `index.html`, poll progress while `done=false`; show a spinner/counter before the folder list loads.
- [ ] Keep egui GUI showing the same counter.

### [ ] 6. Keep-all-subfolders helper
- [ ] Extend `MarkFolderBody` with `recursive: bool`.
- [ ] In `mark_folder`, when recursive, match `parent.starts_with(folder)` instead of `parent == folder`.
- [ ] Add a "Keep this folder + subfolders" checkbox in `index.html` next to the per-folder keep checkbox.
- [ ] Mirror in egui GUI.

## Maybe later

- [ ] Per-file error reporting surfaced in the UI (currently only in the move response JSON + terminal).
- [ ] Configurable hash algorithm (xxhash for speed, sha256 for paranoia) behind a flag.
- [ ] Delete the stale `config.json` in the repo root (superseded by per-user prefs).
- [ ] `.DS_Store` auto-ignore (macOS noise) — add to default `ignore_names`.