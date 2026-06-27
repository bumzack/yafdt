use crate::model::{FileEntry, Folder, Md5};
use glob::Pattern;
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use walkdir::WalkDir;

/// Which hash algorithm to use when detecting duplicates. The digest is only
/// used to compare file equality — never for cryptographic purposes — so the
/// fast non-crypto xxhash is a fine default for large scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgo {
    Md5,
    Xxhash,
    Sha256,
}

impl HashAlgo {
    /// Parse from a CLI string ("md5" | "xxhash" | "sha256"). Case-insensitive.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "md5" => Ok(HashAlgo::Md5),
            "xxhash" | "xxh" | "xxh64" => Ok(HashAlgo::Xxhash),
            "sha256" | "sha2-256" => Ok(HashAlgo::Sha256),
            other => Err(format!(
                "unknown hash '{}': expected md5, xxhash, or sha256",
                other
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HashAlgo::Md5 => "md5",
            HashAlgo::Xxhash => "xxhash",
            HashAlgo::Sha256 => "sha256",
        }
    }
}

/// Stream a file through a BufReader and compute its digest as a hex string.
/// O(1) memory regardless of file size.
fn hash_file(path: &Path, algo: HashAlgo) -> io::Result<Md5> {
    let file = fs::File::open(path)?;
    let mut reader = io::BufReader::with_capacity(64 * 1024, file);
    match algo {
        HashAlgo::Md5 => {
            let mut ctx = md5::Context::new();
            io::copy(&mut reader, &mut ctx)?;
            Ok(format!("{:x}", ctx.compute()))
        }
        HashAlgo::Xxhash => {
            use std::hash::Hasher;
            use xxhash_rust::xxh64::Xxh64;
            let mut hasher = Xxh64::new(0); // seed 0
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.write(&buf[..n]);
            }
            Ok(format!("{:016x}", hasher.finish()))
        }
        HashAlgo::Sha256 => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            io::copy(&mut reader, &mut hasher)?;
            Ok(format!("{:x}", hasher.finalize()))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanFilter {
    pub min_size: u64,
    pub include_globs: Vec<Pattern>,
    pub ignore_name_globs: Vec<Pattern>,
    pub ignore_exts: Vec<String>,
    pub exclude_dirs: Vec<PathBuf>,
    /// Directory name fragments to skip if any path component equals one of
    /// these (e.g. "node_modules", "target").
    pub exclude_components: Vec<String>,
    pub hash_algo: HashAlgo,
}

impl ScanFilter {
    pub fn accepts(&self, path: &Path, meta: &fs::Metadata) -> bool {
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
    pub fn excluded(&self, p: &Path) -> bool {
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
pub fn scan_with_progress(
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
            let hash = match hash_file(&path, filter.hash_algo) {
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
pub fn scan(root: &Path, filter: &ScanFilter) -> Vec<Folder> {
    let walked = AtomicUsize::new(0);
    let hashed = AtomicUsize::new(0);
    scan_with_progress(root, filter, &walked, &hashed)
}
