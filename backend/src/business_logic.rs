use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use crate::error::YafdError;
use crate::models::{AFile, Config, DuplicateFiles};

pub fn find_duplicates(config: &Config) -> Result<Vec<DuplicateFiles>, YafdError> {
    println!("folder: {}", config.root_folder);
    let start = Utc::now();
    let ts = start.timestamp();

    // Compute duplicates here or reuse the precomputed data
    let duplicate_candidates = compute_duplicates(config).unwrap();

    let mut duplicates: Vec<DuplicateFiles> = duplicate_candidates
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|p| {
            let cnt_duplicates = p.1.len();
            DuplicateFiles {
                hash: p.0,
                paths: p.1,
                cnt_duplicates,
            }
        })
        .collect();

    duplicates.sort_by(|a, b| b.cnt_duplicates.cmp(&a.cnt_duplicates));

    let pretty = serde_json::to_string_pretty(&duplicates).expect("should be a json");

    let json_filename = format!("result_{}.json", ts);
    let mut file = File::create_new(&json_filename).expect("should create a file");
    file.write_all(pretty.as_bytes())
        .expect("should write a string");
    file.flush().expect("should flash");

    let duration = Utc::now() - start;
    println!("finding duplicates took {} secs", duration.num_seconds());
    Ok(duplicates)
}

fn compute_duplicates(config: &Config) -> Result<HashMap<String, Vec<AFile>>, YafdError> {
    let mut file_hashes: HashMap<String, Vec<AFile>> = HashMap::new();

    let mut cnt_entries = 0;

    let directory = PathBuf::from(&config.root_folder);

    for entry in walkdir::WalkDir::new(directory) {
        let entry = entry?;

        let path = entry.path().to_str().expect("should be a str");

        let ff = config
            .skip_folders
            .iter()
            .find(|folder| folder.contains(path));

        if entry.file_type().is_file() && ff.is_none() {
            let meta_data = entry.metadata().expect("should have metadata");
            if meta_data.size() > config.min_file_size {
                let fname = entry.file_name().to_str().expect("str");
                let extension = extract_extension(fname);

                if config.consider_extensions.contains(&extension) {
                    cnt_entries += 1;
                    let path = entry.path().to_path_buf();
                    let hash = compute_file_hash(&path)?;
                    let created = meta_data.created().expect("should have a date");
                    let chrono_created: DateTime<Utc> = created.into();
                    let chrono_created = chrono_created.naive_local();
                    let file_name = entry.file_name().to_str().expect("should ").to_string();
                    let file_size = meta_data.size();

                    let a_file = AFile {
                        file_name,
                        file_size,
                        created,
                        chrono_created,
                        path,
                    };

                    file_hashes.entry(hash).or_default().push(a_file);
                }
            }
        }
        if cnt_entries % 100 == 0 {
            println!("{} entries found ", cnt_entries);
        }
    }

    Ok(file_hashes)
}

fn compute_file_hash(path: &PathBuf) -> Result<String, YafdError> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_extension(s: &str) -> String {
    let parts = s.split('.').map(|x| x.to_string()).collect::<Vec<String>>();
    if parts.len() > 1 {
        let ext = &parts[parts.len() - 1];
        if ext.is_empty() {
            "".to_string()
        } else {
            ext.clone()
        }
    } else {
        "".to_string()
    }
}
