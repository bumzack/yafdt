use crate::model::Folder;
use serde::Serialize;
use std::{
    fs,
    io::{self},
    path::{Path, PathBuf},
};

/// One planned move: source file -> destination under target, preserving the
/// full relative path from root.
#[derive(Debug, Clone, Serialize)]
pub struct MovePlanItem {
    pub src: String,
    pub dest: String,
    pub size: u64,
}

/// Resolve a destination that never overwrites an existing file. If `dest`
/// already exists, append `.1`, `.2`, ... to the full filename until a free
/// path is found (e.g. `a.txt` -> `a.txt.1` -> `a.txt.2`). Returns the path.
pub fn unique_dest(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let parent = dest.parent();
    let name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut n = 1u32;
    loop {
        let candidate_name = format!("{}.{}", name, n);
        let candidate = match parent {
            Some(p) => p.join(candidate_name),
            None => PathBuf::from(candidate_name),
        };
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

/// Build the list of files that *would* be moved, without touching the disk.
/// `folders` is read-only here. The kept-folder guard (>=1 kept) is the
/// caller's responsibility.
pub fn plan_move(folders: &[Folder], root: &Path, target: &Path) -> (Vec<MovePlanItem>, Vec<String>) {
    let mut plan: Vec<MovePlanItem> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for fv in folders {
        if fv.keep {
            continue;
        }
        for f in &fv.files {
            let p = Path::new(&f.path);
            let rel = match p.strip_prefix(root) {
                Ok(r) => r,
                Err(_) => {
                    errors.push(format!("not under root: {}", f.path));
                    continue;
                }
            };
            let dest = unique_dest(&target.join(rel));
            plan.push(MovePlanItem {
                src: f.path.clone(),
                dest: dest.to_string_lossy().to_string(),
                size: f.size,
            });
        }
    }
    (plan, errors)
}

/// Execute a previously built plan: move each src to its dest, creating parent
/// dirs as needed. Never overwrites — if the dest exists at execution time,
/// re-resolves with `.1`/`.2`/... Returns (moved_count, per_file_errors).
pub fn execute_move(plan: &[MovePlanItem]) -> (usize, Vec<String>) {
    let mut moved = 0;
    let mut errors: Vec<String> = Vec::new();
    for item in plan {
        let p = Path::new(&item.src);
        // Re-resolve: a previous item in this same batch may have created
        // item.dest already.
        let dest = unique_dest(Path::new(&item.dest));
        if let Some(parent) = dest.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                errors.push(format!("mkdir {} failed: {}", parent.display(), e));
                continue;
            }
        }
        println!("moving ||{}||  -->  ||{}||", p.display(), dest.display());
        if let Err(e) = fs::rename(p, &dest) {
            if e.raw_os_error() == Some(18) /* EXDEV */
                || e.kind() == io::ErrorKind::CrossesDevices
            {
                if let Err(e2) = fs::copy(p, &dest).and_then(|_| fs::remove_file(p)) {
                    errors.push(format!("copy {} failed: {}", item.src, e2));
                    continue;
                }
            } else {
                errors.push(format!("rename {} failed: {}", item.src, e));
                continue;
            }
        }
        moved += 1;
    }
    (moved, errors)
}

/// Move every file in every non-kept folder to target/<rel path from root>.
/// Returns (moved, errors). Errors are per-file so one bad file doesn't abort
/// the whole batch. Also prunes moved files from `folders` so the UI updates.
pub fn move_non_kept(
    folders: &mut Vec<Folder>,
    root: &Path,
    target: &Path,
) -> (usize, Vec<String>) {
    let (plan, mut errors) = plan_move(folders, root, target);
    let (moved, exec_errors) = execute_move(&plan);
    errors.extend(exec_errors);

    // Clear files in non-kept folders (they've been moved); keep kept folders as-is.
    for fv in folders.iter_mut() {
        if !fv.keep {
            fv.files.clear();
            fv.total_size = 0;
        }
    }
    folders.retain(|f| !f.files.is_empty() || f.keep);
    (moved, errors)
}
