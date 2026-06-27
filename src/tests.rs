use crate::*;
use crate::cli::{parse_bytes, human_bytes};
use crate::model::{FileEntry, Folder};
use crate::move_files::{execute_move, move_non_kept, plan_move, unique_dest};
use crate::prefs::Prefs;
use crate::scan::{scan, HashAlgo, ScanFilter};
use glob::Pattern;
use std::{fs, io::Write, path::{Path, PathBuf}};

// ---- parse_bytes ----

#[test]
fn parse_bytes_plain_number() {
    assert_eq!(parse_bytes("1024"), Some(1024));
    assert_eq!(parse_bytes("0"), Some(0));
}

#[test]
fn parse_bytes_b_suffix() {
    assert_eq!(parse_bytes("100B"), Some(100));
    assert_eq!(parse_bytes("100b"), Some(100));
    assert_eq!(parse_bytes("100 B"), Some(100));
}

#[test]
fn parse_bytes_kb_mb_gb() {
    assert_eq!(parse_bytes("1KB"), Some(1024));
    assert_eq!(parse_bytes("1kb"), Some(1024));
    assert_eq!(parse_bytes("1K"), Some(1024));
    assert_eq!(parse_bytes("1MB"), Some(1024 * 1024));
    assert_eq!(parse_bytes("2 MB"), Some(2 * 1024 * 1024));
    assert_eq!(parse_bytes("1GB"), Some(1024 * 1024 * 1024));
    assert_eq!(parse_bytes("1.5GB"), Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64));
}

#[test]
fn parse_bytes_invalid() {
    assert_eq!(parse_bytes("abc"), None);
    assert_eq!(parse_bytes(""), None);
    assert_eq!(parse_bytes("1TB"), None); // unsupported suffix
}

// ---- human_bytes ----

#[test]
fn human_bytes_units() {
    assert_eq!(human_bytes(0), "0 B");
    assert_eq!(human_bytes(512), "512 B");
    assert_eq!(human_bytes(2048), "2.0 KB");
    assert_eq!(human_bytes(1024 * 1024), "1.00 MB");
    assert_eq!(human_bytes(1024 * 1024 * 1024), "1.00 GB");
}

// ---- ScanFilter::excluded ----

fn empty_filter() -> ScanFilter {
    ScanFilter {
        min_size: 0,
        include_globs: vec![],
        ignore_name_globs: vec![],
        ignore_exts: vec![],
        exclude_dirs: vec![],
        exclude_components: vec![],
        hash_algo: HashAlgo::Md5,
    }
}

#[test]
fn excluded_by_dir_prefix() {
    let mut f = empty_filter();
    f.exclude_dirs = vec![PathBuf::from("/Users/foo/secret")];
    assert!(f.excluded(Path::new("/Users/foo/secret/a.txt")));
    assert!(f.excluded(Path::new("/Users/foo/secret/sub/b.txt")));
    assert!(!f.excluded(Path::new("/Users/foo/public/a.txt")));
}

#[test]
fn excluded_by_component_name() {
    let mut f = empty_filter();
    f.exclude_components = vec!["node_modules".into(), "target".into()];
    assert!(f.excluded(Path::new("/proj/node_modules/pkg/index.js")));
    assert!(f.excluded(Path::new("/proj/target/debug/exe")));
    assert!(!f.excluded(Path::new("/proj/src/main.rs")));
    // a file literally named "target" (no extension) is still excluded by component
    assert!(f.excluded(Path::new("/proj/target")));
}

#[test]
fn excluded_neither() {
    let f = empty_filter();
    assert!(!f.excluded(Path::new("/anywhere/file.txt")));
}

// ---- ScanFilter::accepts ----

fn meta_with_size(size: u64) -> fs::Metadata {
    // We can't easily fake Metadata; use a real temp file of the given size.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    if size > 0 {
        let f = fs::OpenOptions::new().write(true).open(tmp.path()).unwrap();
        f.set_len(size).unwrap();
    }
    fs::metadata(tmp.path()).unwrap()
}

#[test]
fn accepts_min_size() {
    let mut f = empty_filter();
    f.min_size = 100;
    assert!(!f.accepts(Path::new("/x/a.txt"), &meta_with_size(50)));
    assert!(f.accepts(Path::new("/x/a.txt"), &meta_with_size(150)));
    assert!(f.accepts(Path::new("/x/a.txt"), &meta_with_size(100))); // boundary inclusive
}

#[test]
fn accepts_ignore_name_glob() {
    let mut f = empty_filter();
    f.ignore_name_globs = vec![Pattern::new("thumb*").unwrap()];
    assert!(!f.accepts(Path::new("/x/thumb_1.jpg"), &meta_with_size(10)));
    assert!(f.accepts(Path::new("/x/photo.jpg"), &meta_with_size(10)));
}

#[test]
fn accepts_ignore_ext() {
    let mut f = empty_filter();
    f.ignore_exts = vec!["log".into(), "tmp".into()];
    assert!(!f.accepts(Path::new("/x/debug.log"), &meta_with_size(10)));
    assert!(!f.accepts(Path::new("/x/cache.TMP"), &meta_with_size(10))); // case-insensitive
    assert!(f.accepts(Path::new("/x/data.json"), &meta_with_size(10)));
}

#[test]
fn accepts_include_glob() {
    let mut f = empty_filter();
    f.include_globs = vec![Pattern::new("*.jpg").unwrap(), Pattern::new("*.png").unwrap()];
    assert!(f.accepts(Path::new("/x/photo.jpg"), &meta_with_size(10)));
    assert!(f.accepts(Path::new("/x/photo.png"), &meta_with_size(10)));
    assert!(!f.accepts(Path::new("/x/photo.gif"), &meta_with_size(10)));
}

// ---- scan (integration) ----

#[test]
fn scan_finds_duplicate_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // /a/x.txt and /b/x.txt have identical content -> duplicates
    // /a/unique.txt is unique -> should not appear
    // /c/sub/x.txt also identical -> third folder with a dup
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::create_dir_all(root.join("c/sub")).unwrap();

    let same = b"hello-world-duplicate";
    for dir in ["a", "b", "c/sub"] {
        let mut f = fs::File::create(root.join(dir).join("x.txt")).unwrap();
        f.write_all(same).unwrap();
    }
    let mut u = fs::File::create(root.join("a/unique.txt")).unwrap();
    u.write_all(b"unique-content-not-a-dup").unwrap();

    let filter = empty_filter();
    let folders = scan(root, &filter);

    // Three folders should appear: a, b, c/sub (each contains a dup of x.txt)
    let folder_names: Vec<String> = folders.iter().map(|f| f.folder.clone()).collect();
    assert_eq!(folders.len(), 3, "expected 3 folders, got {:?}", folder_names);

    // Each folder should list exactly the one duplicate file (x.txt),
    // NOT unique.txt (it was skipped by the size prefilter).
    for fv in &folders {
        assert_eq!(fv.files.len(), 1, "folder {} should have 1 file", fv.folder);
        assert!(fv.files[0].path.ends_with("x.txt"));
    }

    // No folder should be kept by default.
    assert!(folders.iter().all(|f| !f.keep));
}

#[test]
fn scan_no_duplicates_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    let mut f1 = fs::File::create(root.join("a/one.txt")).unwrap();
    f1.write_all(b"content-one").unwrap();
    let mut f2 = fs::File::create(root.join("b/two.txt")).unwrap();
    f2.write_all(b"content-two-different").unwrap();

    let filter = empty_filter();
    let folders = scan(root, &filter);
    assert!(folders.is_empty(), "expected no duplicate folders, got {:?}", folders);
}

// ---- plan_move (integration) ----

#[test]
fn plan_move_excludes_kept_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let target = root.join("target");

    fs::create_dir_all(root.join("keep_me")).unwrap();
    fs::create_dir_all(root.join("move_me")).unwrap();
    let content = b"dup-content";
    let mut a = fs::File::create(root.join("keep_me/f.txt")).unwrap();
    a.write_all(content).unwrap();
    let mut b = fs::File::create(root.join("move_me/f.txt")).unwrap();
    b.write_all(content).unwrap();

    let filter = empty_filter();
    let mut folders = scan(root, &filter);
    // mark keep_me as kept
    for fv in folders.iter_mut() {
        if fv.folder.ends_with("keep_me") {
            fv.keep = true;
        }
    }

    let (plan, errors) = plan_move(&folders, root, &target);
    assert!(errors.is_empty(), "{:?}", errors);
    // only the move_me file should be in the plan
    assert_eq!(plan.len(), 1);
    assert!(plan[0].src.ends_with("move_me/f.txt"));
    assert!(plan[0].dest.ends_with("target/move_me/f.txt"));
}

// ---- prefs union-merge ----

#[test]
fn prefs_default_exclude_components_include_node_modules() {
    let p = Prefs::default_values();
    assert!(p.exclude_components.contains(&"node_modules".to_string()));
    assert!(p.exclude_components.contains(&"target".to_string()));
    assert!(p.exclude_components.contains(&".git".to_string()));
}

#[test]
fn prefs_default_bind_and_open_browser() {
    let p = Prefs::default_values();
    assert_eq!(p.bind, "127.0.0.1:8787");
    assert!(p.open_browser);
}

// ---- unique_dest (never overwrite) ----

#[test]
fn unique_dest_no_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("a.txt");
    assert_eq!(unique_dest(&dest), dest);
}

#[test]
fn unique_dest_with_extension() {
    let tmp = tempfile::tempdir().unwrap();
    // pre-create a.txt; the unique dest should be a.txt.1
    fs::write(tmp.path().join("a.txt"), b"x").unwrap();
    let dest = unique_dest(&tmp.path().join("a.txt"));
    assert_eq!(dest.file_name().unwrap(), "a.txt.1");
    // pre-create a.txt.1 too; should get a.txt.2
    fs::write(tmp.path().join("a.txt.1"), b"x").unwrap();
    let dest2 = unique_dest(&tmp.path().join("a.txt"));
    assert_eq!(dest2.file_name().unwrap(), "a.txt.2");
}

#[test]
fn unique_dest_no_extension() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("Makefile"), b"x").unwrap();
    let dest = unique_dest(&tmp.path().join("Makefile"));
    assert_eq!(dest.file_name().unwrap(), "Makefile.1");
}

// ---- target folder is auto-ignored during scan ----

#[test]
fn scan_excludes_target_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let target = root.join("moved_here");

    // two genuine duplicates outside target
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/x.txt"), b"dup").unwrap();
    fs::write(root.join("b/x.txt"), b"dup").unwrap();

    // a file ALREADY inside target that matches the duplicates' content.
    // If target is auto-excluded, it won't appear as a third copy.
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("x.txt"), b"dup").unwrap();

    let mut filter = empty_filter();
    filter.exclude_dirs.push(target.clone());
    let folders = scan(root, &filter);

    // Only folders a and b should appear (target/x.txt is excluded).
    assert_eq!(folders.len(), 2);
    assert!(folders.iter().all(|f| !f.folder.ends_with("moved_here")));
}

// ---- move_non_kept: non-overwrite + target structure (e2e) ----

/// End-to-end test mirroring the real test_data layout:
///   prio/Cargo.toml, excluded/Cargo.toml, to_delete/prionon/Cargo.toml
/// all identical. Keep `excluded`, move the rest to target, verify the
/// target tree preserves relative paths and the kept folder is untouched.
/// Then run a SECOND move after recreating the sources and confirm the
/// .1/.2 non-overwrite suffix kicks in (target already has the files).
#[test]
fn e2e_move_preserves_paths_and_no_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let target = root.join("moved");

    // build three identical files in three folders (like test_data)
    for dir in ["prio", "excluded", "to_delete/prionon"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let cargo = b"[package]\nname=\"x\"\nversion=\"0.1.0\"\n[dependencies]\n";
    let main = b"fn main(){}\n";
    fs::write(root.join("prio/Cargo.toml"), cargo).unwrap();
    fs::write(root.join("excluded/Cargo.toml"), cargo).unwrap();
    fs::write(root.join("to_delete/prionon/Cargo.toml"), cargo).unwrap();
    fs::write(root.join("prio/main.rs"), main).unwrap();
    fs::write(root.join("excluded/main.rs"), main).unwrap();
    fs::write(root.join("to_delete/prionon/main.rs"), main).unwrap();

    let mut filter = empty_filter();
    filter.exclude_dirs.push(target.clone());

    // ---- first move: keep `excluded` ----
    let mut folders = scan(root, &filter);
    assert_eq!(folders.len(), 3, "expected prio/excluded/to_delete(prionon)");
    for fv in folders.iter_mut() {
        if fv.folder.ends_with("excluded") {
            fv.keep = true;
        }
    }
    let (moved, errors) = move_non_kept(&mut folders, root, &target);
    assert!(errors.is_empty(), "{:?}", errors);
    assert_eq!(moved, 4, "prio (Cargo.toml, main.rs) + to_delete/prionon (Cargo.toml, main.rs)");

    // target should mirror the relative paths
    assert!(target.join("prio/Cargo.toml").exists());
    assert!(target.join("prio/main.rs").exists());
    assert!(target.join("to_delete/prionon/Cargo.toml").exists());
    assert!(target.join("to_delete/prionon/main.rs").exists());

    // kept folder untouched
    assert!(root.join("excluded/Cargo.toml").exists());
    assert!(root.join("excluded/main.rs").exists());
    // moved-away files gone from source
    assert!(!root.join("prio/Cargo.toml").exists());
    assert!(!root.join("to_delete/prionon/main.rs").exists());

    // ---- second move: recreate sources, keep excluded again ----
    fs::write(root.join("prio/Cargo.toml"), cargo).unwrap();
    fs::write(root.join("to_delete/prionon/Cargo.toml"), cargo).unwrap();
    fs::write(root.join("prio/main.rs"), main).unwrap();
    fs::write(root.join("to_delete/prionon/main.rs"), main).unwrap();

    let mut folders2 = scan(root, &filter);
    for fv in folders2.iter_mut() {
        if fv.folder.ends_with("excluded") {
            fv.keep = true;
        }
    }
    let (moved2, errors2) = move_non_kept(&mut folders2, root, &target);
    assert!(errors2.is_empty(), "{:?}", errors2);
    assert_eq!(moved2, 4);

    // Non-overwrite: original target files still present, plus .1 suffixed
    assert!(target.join("prio/Cargo.toml").exists());       // original
    assert!(target.join("prio/Cargo.toml.1").exists());       // second move
    assert!(target.join("to_delete/prionon/main.rs").exists());
    assert!(target.join("to_delete/prionon/main.rs.1").exists());
}

// ---- plan_move previews the non-overwrite dest too ----

#[test]
fn plan_move_uses_unique_dest_when_target_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let target = root.join("moved");

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/f.txt"), b"dup").unwrap();
    // pre-populate the target so the planned dest must shift to .1
    fs::create_dir_all(target.join("src")).unwrap();
    fs::write(target.join("src/f.txt"), b"existing").unwrap();

    let folders = vec![Folder {
        folder: root.join("src").to_string_lossy().to_string(),
        keep: false,
        files: vec![FileEntry {
            path: root.join("src/f.txt").to_string_lossy().to_string(),
            size: 3,
        }],
        total_size: 3,
    }];

    let (plan, errors) = plan_move(&folders, root, &target);
    assert!(errors.is_empty(), "{:?}", errors);
    assert_eq!(plan.len(), 1);
    assert!(plan[0].dest.ends_with("moved/src/f.txt.1"), "got {}", plan[0].dest);
}

// ---- move errors are collected per-file, batch continues ----

#[test]
fn move_collects_errors_for_missing_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let target = root.join("moved");

    // one real file + one that doesn't exist (simulated by a plan entry
    // pointing at a nonexistent src)
    let folders = vec![Folder {
        folder: root.join("src").to_string_lossy().to_string(),
        keep: false,
        files: vec![
            FileEntry {
                path: root.join("src/real.txt").to_string_lossy().to_string(),
                size: 3,
            },
            FileEntry {
                path: root.join("src/ghost.txt").to_string_lossy().to_string(),
                size: 0,
            },
        ],
        total_size: 3,
    }];
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/real.txt"), b"abc").unwrap();
    // deliberately do NOT create ghost.txt

    let (plan, _) = plan_move(&folders, root, &target);
    let (moved, errors) = execute_move(&plan);
    assert_eq!(moved, 1, "real.txt should move");
    assert_eq!(errors.len(), 1, "ghost.txt should error");
    assert!(errors[0].contains("ghost.txt"), "{}", errors[0]);
}

// ---- HashAlgo parsing ----

#[test]
fn hash_algo_parse_valid() {
    assert_eq!(HashAlgo::parse("md5"), Ok(HashAlgo::Md5));
    assert_eq!(HashAlgo::parse("MD5"), Ok(HashAlgo::Md5));
    assert_eq!(HashAlgo::parse("xxhash"), Ok(HashAlgo::Xxhash));
    assert_eq!(HashAlgo::parse("xxh64"), Ok(HashAlgo::Xxhash));
    assert_eq!(HashAlgo::parse("sha256"), Ok(HashAlgo::Sha256));
    assert_eq!(HashAlgo::parse(" SHA256 "), Ok(HashAlgo::Sha256));
}

#[test]
fn hash_algo_parse_invalid() {
    assert!(HashAlgo::parse("crc32").is_err());
    assert!(HashAlgo::parse("").is_err());
    assert!(HashAlgo::parse("sha1").is_err());
}

#[test]
fn hash_algo_as_str() {
    assert_eq!(HashAlgo::Md5.as_str(), "md5");
    assert_eq!(HashAlgo::Xxhash.as_str(), "xxhash");
    assert_eq!(HashAlgo::Sha256.as_str(), "sha256");
}

// ---- all three hash algorithms detect the same duplicates ----

#[test]
fn all_hash_algos_find_same_duplicates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(root.join("b")).unwrap();
    fs::write(root.join("a/dup.txt"), b"identical-content").unwrap();
    fs::write(root.join("b/dup.txt"), b"identical-content").unwrap();
    // unique file so the size prefilter still runs
    fs::write(root.join("a/uniq.txt"), b"unique").unwrap();

    for algo in [HashAlgo::Md5, HashAlgo::Xxhash, HashAlgo::Sha256] {
        let mut filter = empty_filter();
        filter.hash_algo = algo;
        let folders = scan(root, &filter);
        assert_eq!(
            folders.len(),
            2,
            "algo {:?} found {:?} folders",
            algo,
            folders.iter().map(|f| &f.folder).collect::<Vec<_>>()
        );
    }
}

// ---- .DS_Store is ignored by default ----

#[test]
fn prefs_default_ignore_names_include_ds_store() {
    let p = Prefs::default_values();
    assert!(p.ignore_names.contains(&".DS_Store".to_string()));
    assert!(p.ignore_names.contains(&"._*".to_string()));
    assert!(p.ignore_names.contains(&"Thumbs.db".to_string()));
    assert!(p.ignore_names.contains(&"desktop.ini".to_string()));
}
