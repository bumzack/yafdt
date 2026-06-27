# TODO — Duplicate Finder

**Status: COMPLETE.** All planned work is done. See [`PROJECT_STATUS.md`](./PROJECT_STATUS.md) for the full picture.

## Roadmap (all done)

- [x] **1. Hashing performance** — size prefilter (bucket by size, only hash groups >1) + streaming `BufReader` hash (O(1) memory)
- [x] **2. Dry-run / preview** — `--dry-run` CLI flag; `POST /api/preview`; web UI "Preview move" button
- [x] **3. Vendor assets** — bootstrap/jQuery embedded via `rust-embed`, works offline
- [x] **4. Tests** — 29 tests (unit + integration + e2e)
- [x] **5. Live scan progress** — background scan thread; `GET /api/scan_progress`; web UI spinner + live counter
- [x] **6. Keep-all-subfolders** — `mark_folder` cascades recursively

## Safety hardening (all done)

- [x] Auto-ignore target folder during scan
- [x] Never overwrite (`.1`/`.2`/... suffix via `unique_dest`)
- [x] Per-file error UI (inline `<ul>` panel, batch continues)

## Polish (all done)

- [x] Configurable hash (`--hash md5|xxhash|sha256`)
- [x] Deleted stale `config.json` (superseded by per-user prefs)
- [x] OS-noise auto-ignore (`.DS_Store`, `._*`, `Thumbs.db`, `desktop.ini`)

## Open

Nothing. If reviving the project, see the "Ideas / Future directions" section of [`PROJECT_STATUS.md`](./PROJECT_STATUS.md) for optional stretch work.